use crate::error::CodegenError;
use crate::ir::*;
use lattice_runtime::node::Value;
use std::collections::HashMap;

pub struct Interpreter {
    stack: Vec<Value>,
    variables: HashMap<String, Value>,
    /// Persistent global variables that survive across multiple executions (for REPL).
    globals: HashMap<String, Value>,
    builtins: HashMap<String, Box<dyn Fn(Vec<Value>) -> Result<Value, CodegenError>>>,
    /// Persistent user-defined functions that survive across multiple executions (for REPL).
    persistent_functions: Vec<Function>,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            variables: HashMap::new(),
            globals: HashMap::new(),
            builtins: HashMap::new(),
            persistent_functions: Vec::new(),
        }
    }

    /// Register a built-in function.
    pub fn register_builtin(
        &mut self,
        name: String,
        f: impl Fn(Vec<Value>) -> Result<Value, CodegenError> + 'static,
    ) {
        self.builtins.insert(name, Box::new(f));
    }

    /// Register standard library built-in functions.
    pub fn register_stdlib(&mut self) {
        self.register_builtin("length".into(), |args| {
            match args.first() {
                Some(Value::Array(arr)) => Ok(Value::Int(arr.len() as i64)),
                Some(Value::String(s)) => Ok(Value::Int(s.len() as i64)),
                _ => Err(CodegenError::TypeError("length requires array or string".into())),
            }
        });
        self.register_builtin("head".into(), |args| {
            match args.first() {
                Some(Value::Array(arr)) => Ok(arr.first().cloned().unwrap_or(Value::Null)),
                _ => Err(CodegenError::TypeError("head requires array".into())),
            }
        });
        self.register_builtin("tail".into(), |args| {
            match args.first() {
                Some(Value::Array(arr)) if !arr.is_empty() => {
                    Ok(Value::Array(arr[1..].to_vec()))
                }
                Some(Value::Array(_)) => Ok(Value::Array(vec![])),
                _ => Err(CodegenError::TypeError("tail requires array".into())),
            }
        });
        self.register_builtin("push".into(), |args| {
            if args.len() != 2 {
                return Err(CodegenError::TypeError("push requires 2 arguments".into()));
            }
            match &args[0] {
                Value::Array(arr) => {
                    let mut new_arr = arr.clone();
                    new_arr.push(args[1].clone());
                    Ok(Value::Array(new_arr))
                }
                _ => Err(CodegenError::TypeError("push requires array as first argument".into())),
            }
        });
        self.register_builtin("reverse".into(), |args| {
            match args.first() {
                Some(Value::Array(arr)) => {
                    let mut rev = arr.clone();
                    rev.reverse();
                    Ok(Value::Array(rev))
                }
                _ => Err(CodegenError::TypeError("reverse requires array".into())),
            }
        });
        self.register_builtin("contains".into(), |args| {
            if args.len() != 2 {
                return Err(CodegenError::TypeError("contains requires 2 arguments".into()));
            }
            match &args[0] {
                Value::Array(arr) => Ok(Value::Bool(arr.contains(&args[1]))),
                _ => Err(CodegenError::TypeError("contains requires array".into())),
            }
        });
        self.register_builtin("range".into(), |args| {
            match (args.first(), args.get(1)) {
                (Some(Value::Int(start)), Some(Value::Int(end))) => {
                    let arr: Vec<Value> = (*start..*end).map(Value::Int).collect();
                    Ok(Value::Array(arr))
                }
                _ => Err(CodegenError::TypeError("range requires two int arguments".into())),
            }
        });
        self.register_builtin("toString".into(), |args| {
            match args.first() {
                Some(Value::Int(n)) => Ok(Value::String(n.to_string())),
                Some(Value::Float(f)) => Ok(Value::String(f.to_string())),
                Some(Value::Bool(b)) => Ok(Value::String(b.to_string())),
                Some(Value::String(s)) => Ok(Value::String(s.clone())),
                Some(Value::Null) => Ok(Value::String("null".to_string())),
                _ => Ok(Value::String("<value>".to_string())),
            }
        });
        // Note: map, filter, fold, flatMap need access to the interpreter
        // and program, so they are handled specially in the Call instruction.
        // We register placeholder builtins that will be intercepted.
        self.register_builtin("print".into(), |args| {
            if let Some(val) = args.first() {
                match val {
                    Value::String(s) => eprintln!("{s}"),
                    other => eprintln!("{other:?}"),
                }
            }
            Ok(Value::Null)
        });
    }

    /// Execute a compiled program, returning the final value.
    pub fn execute(&mut self, program: &Program) -> Result<Value, CodegenError> {
        let entry = program.functions[program.entry].clone();
        self.run_function(entry, vec![], program)
    }

    /// Execute a compiled program, persisting top-level variables and functions for REPL use.
    pub fn execute_persistent(&mut self, program: &Program) -> Result<Value, CodegenError> {
        // Save non-entry, non-lambda functions to persistent storage for cross-line calls
        for (i, func) in program.functions.iter().enumerate() {
            if i != program.entry && !func.name.starts_with("__lambda_") {
                // Replace existing function with same name, or add new
                if let Some(existing) = self.persistent_functions.iter_mut().find(|f| f.name == func.name) {
                    *existing = func.clone();
                } else {
                    self.persistent_functions.push(func.clone());
                }
            }
        }
        let entry = program.functions[program.entry].clone();
        self.run_function_persistent(entry, program)
    }

    /// Set a variable in the interpreter's local scope before execution.
    pub fn set_variable(&mut self, name: impl Into<String>, value: Value) {
        self.variables.insert(name.into(), value);
    }

    /// Returns a reference to the persistent global variables.
    pub fn globals(&self) -> &HashMap<String, Value> {
        &self.globals
    }

    fn pop(&mut self) -> Result<Value, CodegenError> {
        self.stack.pop().ok_or(CodegenError::StackUnderflow)
    }

    /// Try to handle higher-order builtin functions (map, filter, fold, flatMap, forEach, any, all).
    /// Returns Some(result) if the name matches, None otherwise.
    fn try_hof_builtin(
        &mut self,
        name: &str,
        args: &[Value],
        program: &Program,
    ) -> Result<Option<Value>, CodegenError> {
        match name {
            "map" => {
                if args.len() != 2 {
                    return Err(CodegenError::TypeError("map requires 2 arguments (array, fn)".into()));
                }
                let arr = match &args[0] {
                    Value::Array(a) => a.clone(),
                    _ => return Err(CodegenError::TypeError("map: first argument must be array".into())),
                };
                let closure = args[1].clone();
                let mut result = Vec::with_capacity(arr.len());
                for elem in arr {
                    let val = self.call_closure_value(closure.clone(), vec![elem], program)?;
                    result.push(val);
                }
                Ok(Some(Value::Array(result)))
            }
            "filter" => {
                if args.len() != 2 {
                    return Err(CodegenError::TypeError("filter requires 2 arguments (array, fn)".into()));
                }
                let arr = match &args[0] {
                    Value::Array(a) => a.clone(),
                    _ => return Err(CodegenError::TypeError("filter: first argument must be array".into())),
                };
                let closure = args[1].clone();
                let mut result = Vec::new();
                for elem in arr {
                    let keep = self.call_closure_value(closure.clone(), vec![elem.clone()], program)?;
                    if matches!(keep, Value::Bool(true)) {
                        result.push(elem);
                    }
                }
                Ok(Some(Value::Array(result)))
            }
            "fold" => {
                if args.len() != 3 {
                    return Err(CodegenError::TypeError("fold requires 3 arguments (array, init, fn)".into()));
                }
                let arr = match &args[0] {
                    Value::Array(a) => a.clone(),
                    _ => return Err(CodegenError::TypeError("fold: first argument must be array".into())),
                };
                let mut acc = args[1].clone();
                let closure = args[2].clone();
                for elem in arr {
                    acc = self.call_closure_value(closure.clone(), vec![acc, elem], program)?;
                }
                Ok(Some(acc))
            }
            "flatMap" => {
                if args.len() != 2 {
                    return Err(CodegenError::TypeError("flatMap requires 2 arguments (array, fn)".into()));
                }
                let arr = match &args[0] {
                    Value::Array(a) => a.clone(),
                    _ => return Err(CodegenError::TypeError("flatMap: first argument must be array".into())),
                };
                let closure = args[1].clone();
                let mut result = Vec::new();
                for elem in arr {
                    let val = self.call_closure_value(closure.clone(), vec![elem], program)?;
                    match val {
                        Value::Array(inner) => result.extend(inner),
                        other => result.push(other),
                    }
                }
                Ok(Some(Value::Array(result)))
            }
            "forEach" => {
                if args.len() != 2 {
                    return Err(CodegenError::TypeError("forEach requires 2 arguments (array, fn)".into()));
                }
                let arr = match &args[0] {
                    Value::Array(a) => a.clone(),
                    _ => return Err(CodegenError::TypeError("forEach: first argument must be array".into())),
                };
                let closure = args[1].clone();
                for elem in arr {
                    self.call_closure_value(closure.clone(), vec![elem], program)?;
                }
                Ok(Some(Value::Null))
            }
            "any" => {
                if args.len() != 2 {
                    return Err(CodegenError::TypeError("any requires 2 arguments (array, fn)".into()));
                }
                let arr = match &args[0] {
                    Value::Array(a) => a.clone(),
                    _ => return Err(CodegenError::TypeError("any: first argument must be array".into())),
                };
                let closure = args[1].clone();
                for elem in arr {
                    let val = self.call_closure_value(closure.clone(), vec![elem], program)?;
                    if matches!(val, Value::Bool(true)) {
                        return Ok(Some(Value::Bool(true)));
                    }
                }
                Ok(Some(Value::Bool(false)))
            }
            "all" => {
                if args.len() != 2 {
                    return Err(CodegenError::TypeError("all requires 2 arguments (array, fn)".into()));
                }
                let arr = match &args[0] {
                    Value::Array(a) => a.clone(),
                    _ => return Err(CodegenError::TypeError("all: first argument must be array".into())),
                };
                let closure = args[1].clone();
                for elem in arr {
                    let val = self.call_closure_value(closure.clone(), vec![elem], program)?;
                    if !matches!(val, Value::Bool(true)) {
                        return Ok(Some(Value::Bool(false)));
                    }
                }
                Ok(Some(Value::Bool(true)))
            }
            // Relational algebra builtins
            "__rel_select__" => {
                // __rel_select__(relation, predicate_closure)
                if args.len() != 2 {
                    return Err(CodegenError::TypeError("select requires 2 arguments".into()));
                }
                let rows = match &args[0] {
                    Value::Array(a) => a.clone(),
                    _ => return Err(CodegenError::TypeError("select: first argument must be array of records".into())),
                };
                let closure = args[1].clone();
                let mut result = Vec::new();
                for row in rows {
                    let keep = self.call_closure_value(closure.clone(), vec![row.clone()], program)?;
                    if matches!(keep, Value::Bool(true)) {
                        result.push(row);
                    }
                }
                Ok(Some(Value::Array(result)))
            }
            "__rel_project__" => {
                // __rel_project__(relation, fields_array)
                if args.len() != 2 {
                    return Err(CodegenError::TypeError("project requires 2 arguments".into()));
                }
                let rows = match &args[0] {
                    Value::Array(a) => a.clone(),
                    _ => return Err(CodegenError::TypeError("project: first argument must be array of records".into())),
                };
                let fields = match &args[1] {
                    Value::Array(a) => a.iter().filter_map(|v| {
                        if let Value::String(s) = v { Some(s.clone()) } else { None }
                    }).collect::<Vec<_>>(),
                    _ => return Err(CodegenError::TypeError("project: second argument must be array of field names".into())),
                };
                let result: Vec<Value> = rows.into_iter().map(|row| {
                    if let Value::Object(obj) = row {
                        let projected: HashMap<String, Value> = fields.iter()
                            .filter_map(|f| obj.get(f).map(|v| (f.clone(), v.clone())))
                            .collect();
                        Value::Object(projected)
                    } else {
                        row
                    }
                }).collect();
                Ok(Some(Value::Array(result)))
            }
            "__rel_join__" => {
                // __rel_join__(left, right, condition_closure)
                if args.len() != 3 {
                    return Err(CodegenError::TypeError("join requires 3 arguments".into()));
                }
                let left_rows = match &args[0] {
                    Value::Array(a) => a.clone(),
                    _ => return Err(CodegenError::TypeError("join: first argument must be array".into())),
                };
                let right_rows = match &args[1] {
                    Value::Array(a) => a.clone(),
                    _ => return Err(CodegenError::TypeError("join: second argument must be array".into())),
                };
                let closure = args[2].clone();
                let mut result = Vec::new();
                for l in &left_rows {
                    for r in &right_rows {
                        // Merge left and right records
                        let merged = match (l, r) {
                            (Value::Object(lo), Value::Object(ro)) => {
                                let mut m = lo.clone();
                                m.extend(ro.iter().map(|(k, v)| (k.clone(), v.clone())));
                                Value::Object(m)
                            }
                            _ => Value::Array(vec![l.clone(), r.clone()]),
                        };
                        let keep = self.call_closure_value(closure.clone(), vec![merged.clone()], program)?;
                        if matches!(keep, Value::Bool(true)) {
                            result.push(merged);
                        }
                    }
                }
                Ok(Some(Value::Array(result)))
            }
            "__rel_group_by__" => {
                // __rel_group_by__(relation, keys_array, agg_closure)
                if args.len() != 3 {
                    return Err(CodegenError::TypeError("group_by requires 3 arguments".into()));
                }
                let rows = match &args[0] {
                    Value::Array(a) => a.clone(),
                    _ => return Err(CodegenError::TypeError("group_by: first argument must be array".into())),
                };
                let keys = match &args[1] {
                    Value::Array(a) => a.iter().filter_map(|v| {
                        if let Value::String(s) = v { Some(s.clone()) } else { None }
                    }).collect::<Vec<_>>(),
                    _ => return Err(CodegenError::TypeError("group_by: second argument must be array of key names".into())),
                };
                // Group rows by key values
                let mut groups: std::collections::HashMap<String, Vec<Value>> = std::collections::HashMap::new();
                for row in &rows {
                    let key_vals: Vec<String> = keys.iter().map(|k| {
                        if let Value::Object(obj) = row {
                            obj.get(k).map(|v| format!("{v:?}")).unwrap_or_default()
                        } else {
                            String::new()
                        }
                    }).collect();
                    let group_key = key_vals.join("|");
                    groups.entry(group_key).or_default().push(row.clone());
                }
                // Apply aggregate closure to each group, or return groups as arrays
                let closure = args[2].clone();
                let mut result = Vec::new();
                for (_key, group) in groups {
                    if matches!(closure, Value::Null) {
                        result.push(Value::Array(group));
                    } else {
                        let val = self.call_closure_value(closure.clone(), vec![Value::Array(group)], program)?;
                        result.push(val);
                    }
                }
                Ok(Some(Value::Array(result)))
            }
            _ => Ok(None),
        }
    }

    fn call_closure_value(
        &mut self,
        closure: Value,
        args: Vec<Value>,
        program: &Program,
    ) -> Result<Value, CodegenError> {
        match closure {
            Value::Object(map) => {
                let func_idx = map
                    .get("__closure_func_idx")
                    .and_then(|v| v.as_int())
                    .ok_or_else(|| CodegenError::TypeError("not a closure".into()))?
                    as usize;
                let callee = program
                    .functions
                    .get(func_idx)
                    .ok_or_else(|| CodegenError::TypeError("invalid closure function index".into()))?
                    .clone();
                let saved = std::mem::take(&mut self.variables);
                for (k, v) in &map {
                    if let Some(name) = k.strip_prefix("__capture_") {
                        self.variables.insert(name.to_string(), v.clone());
                    }
                }
                let result = self.run_function(callee, args, program)?;
                self.variables = saved;
                Ok(result)
            }
            _ => Err(CodegenError::TypeError("not a callable value".into())),
        }
    }

    fn run_function(
        &mut self,
        func: Function,
        args: Vec<Value>,
        program: &Program,
    ) -> Result<Value, CodegenError> {
        let old_vars = std::mem::take(&mut self.variables);

        for (name, val) in func.params.iter().zip(args) {
            self.variables.insert(name.clone(), val);
        }

        let mut pc = 0;
        while pc < func.instructions.len() {
            match &func.instructions[pc] {
                // ── Constants ────────────────────────
                Instruction::PushInt(n) => self.stack.push(Value::Int(*n)),
                Instruction::PushFloat(f) => self.stack.push(Value::Float(*f)),
                Instruction::PushBool(b) => self.stack.push(Value::Bool(*b)),
                Instruction::PushString(s) => self.stack.push(Value::String(s.clone())),
                Instruction::PushNull => self.stack.push(Value::Null),

                // ── Variables ────────────────────────
                Instruction::LoadVar(name) => {
                    let val = self
                        .variables
                        .get(name)
                        .or_else(|| old_vars.get(name))
                        .or_else(|| self.globals.get(name))
                        .ok_or_else(|| CodegenError::UndefinedVariable(name.clone()))?
                        .clone();
                    self.stack.push(val);
                }
                Instruction::StoreVar(name) => {
                    let val = self.pop()?;
                    self.variables.insert(name.clone(), val);
                }

                // ── Arithmetic ──────────────────────
                Instruction::Add => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(arith_add(a, b)?);
                }
                Instruction::Sub => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(arith_sub(a, b)?);
                }
                Instruction::Mul => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(arith_mul(a, b)?);
                }
                Instruction::Div => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(arith_div(a, b)?);
                }
                Instruction::Mod => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(arith_mod(a, b)?);
                }
                Instruction::Concat => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    match (a, b) {
                        (Value::String(x), Value::String(y)) => {
                            self.stack.push(Value::String(format!("{x}{y}")));
                        }
                        (Value::Array(mut x), Value::Array(y)) => {
                            x.extend(y);
                            self.stack.push(Value::Array(x));
                        }
                        _ => {
                            return Err(CodegenError::TypeError(
                                "++ requires string or array operands".into(),
                            ))
                        }
                    }
                }
                Instruction::Neg => {
                    let a = self.pop()?;
                    let result = match a {
                        Value::Int(x) => Value::Int(-x),
                        Value::Float(x) => Value::Float(-x),
                        _ => {
                            return Err(CodegenError::TypeError(
                                "negation requires numeric operand".into(),
                            ))
                        }
                    };
                    self.stack.push(result);
                }

                // ── Comparison ──────────────────────
                Instruction::Eq => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(Value::Bool(values_eq(&a, &b)));
                }
                Instruction::Neq => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(Value::Bool(!values_eq(&a, &b)));
                }
                Instruction::Lt => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(Value::Bool(compare_values(&a, &b)? < 0));
                }
                Instruction::Gt => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(Value::Bool(compare_values(&a, &b)? > 0));
                }
                Instruction::Leq => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(Value::Bool(compare_values(&a, &b)? <= 0));
                }
                Instruction::Geq => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(Value::Bool(compare_values(&a, &b)? >= 0));
                }

                // ── Logic ───────────────────────────
                Instruction::And => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    match (a, b) {
                        (Value::Bool(x), Value::Bool(y)) => {
                            self.stack.push(Value::Bool(x && y));
                        }
                        _ => {
                            return Err(CodegenError::TypeError(
                                "logical AND requires bool operands".into(),
                            ))
                        }
                    }
                }
                Instruction::Or => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    match (a, b) {
                        (Value::Bool(x), Value::Bool(y)) => {
                            self.stack.push(Value::Bool(x || y));
                        }
                        _ => {
                            return Err(CodegenError::TypeError(
                                "logical OR requires bool operands".into(),
                            ))
                        }
                    }
                }
                Instruction::Not => {
                    let a = self.pop()?;
                    match a {
                        Value::Bool(x) => self.stack.push(Value::Bool(!x)),
                        _ => {
                            return Err(CodegenError::TypeError(
                                "logical NOT requires bool operand".into(),
                            ))
                        }
                    }
                }

                // ── Stack manipulation ──────────────
                Instruction::Dup => {
                    let val = self
                        .stack
                        .last()
                        .ok_or(CodegenError::StackUnderflow)?
                        .clone();
                    self.stack.push(val);
                }
                Instruction::Pop => {
                    self.pop()?;
                }
                Instruction::Swap => {
                    let len = self.stack.len();
                    if len < 2 {
                        return Err(CodegenError::StackUnderflow);
                    }
                    self.stack.swap(len - 1, len - 2);
                }

                // ── Control flow ────────────────────
                Instruction::Jump(target) => {
                    pc = *target;
                    continue;
                }
                Instruction::JumpIfFalse(target) => {
                    let val = self.pop()?;
                    match val {
                        Value::Bool(false) => {
                            pc = *target;
                            continue;
                        }
                        Value::Bool(true) => {} // fall through
                        _ => {
                            return Err(CodegenError::TypeError(
                                "conditional requires bool value".into(),
                            ))
                        }
                    }
                }

                // ── Functions ───────────────────────
                Instruction::Call(name, arg_count) => {
                    let name = name.clone();
                    let arg_count = *arg_count;

                    let mut call_args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        call_args.push(self.pop()?);
                    }
                    call_args.reverse();

                    if let Some(result) = self.try_hof_builtin(&name, &call_args, program)? {
                        self.stack.push(result);
                    } else if self.builtins.contains_key(&name) {
                        let result = (self.builtins[&name])(call_args)?;
                        self.stack.push(result);
                    } else if let Some(callee) =
                        program.functions.iter().find(|f| f.name == name)
                            .or_else(|| self.persistent_functions.iter().find(|f| f.name == name))
                    {
                        let callee = callee.clone();
                        let result = self.run_function(callee, call_args, program)?;
                        self.stack.push(result);
                    } else if let Some(closure_val) = self.variables.get(&name)
                        .or_else(|| old_vars.get(&name))
                        .or_else(|| self.globals.get(&name))
                        .cloned()
                    {
                        let result = self.call_closure_value(closure_val, call_args, program)?;
                        self.stack.push(result);
                    } else {
                        return Err(CodegenError::UndefinedVariable(name));
                    }
                }
                Instruction::Return => break,

                // ── Data structures ─────────────────
                Instruction::GetField(name) => {
                    let val = self.pop()?;
                    match val {
                        Value::Object(map) => {
                            let field_val = map
                                .get(name)
                                .ok_or_else(|| {
                                    CodegenError::TypeError(format!(
                                        "no field '{name}' in record"
                                    ))
                                })?
                                .clone();
                            self.stack.push(field_val);
                        }
                        _ => {
                            return Err(CodegenError::TypeError(
                                "field access on non-record".into(),
                            ))
                        }
                    }
                }
                Instruction::MakeArray(count) => {
                    let count = *count;
                    let mut elements = Vec::with_capacity(count);
                    for _ in 0..count {
                        elements.push(self.pop()?);
                    }
                    elements.reverse();
                    self.stack.push(Value::Array(elements));
                }
                Instruction::MakeRecord(fields) => {
                    let fields = fields.clone();
                    let mut values = Vec::with_capacity(fields.len());
                    for _ in 0..fields.len() {
                        values.push(self.pop()?);
                    }
                    values.reverse();
                    let map: HashMap<String, Value> =
                        fields.into_iter().zip(values).collect();
                    self.stack.push(Value::Object(map));
                }

                // ── Pattern matching ────────────────
                Instruction::TestInt(n) => {
                    let val = self.stack.last().ok_or(CodegenError::StackUnderflow)?;
                    let matches = matches!(val, Value::Int(v) if v == n);
                    self.stack.push(Value::Bool(matches));
                }
                Instruction::TestString(s) => {
                    let val = self.stack.last().ok_or(CodegenError::StackUnderflow)?;
                    let matches = matches!(val, Value::String(v) if v == s);
                    self.stack.push(Value::Bool(matches));
                }
                Instruction::TestBool(b) => {
                    let val = self.stack.last().ok_or(CodegenError::StackUnderflow)?;
                    let matches = matches!(val, Value::Bool(v) if v == b);
                    self.stack.push(Value::Bool(matches));
                }
                Instruction::TestConstructor(name) => {
                    let val = self.stack.last().ok_or(CodegenError::StackUnderflow)?;
                    let matches = match val {
                        Value::Constructor { name: n, .. } => n == name,
                        Value::Object(map) => map.get("__constructor").map_or(false, |v| {
                            matches!(v, Value::String(n) if n == name)
                        }),
                        _ => false,
                    };
                    self.stack.push(Value::Bool(matches));
                }
                Instruction::ExtractField(idx) => {
                    let val = self.pop()?;
                    match val {
                        Value::Constructor { fields, .. } => {
                            self.stack.push(fields.get(*idx).cloned().unwrap_or(Value::Null));
                        }
                        Value::Object(map) => {
                            let field_val = map
                                .get(&format!("__{idx}"))
                                .or_else(|| {
                                    let mut keys: Vec<_> = map.keys()
                                        .filter(|k| !k.starts_with("__"))
                                        .collect();
                                    keys.sort();
                                    keys.get(*idx).and_then(|k| map.get(*k))
                                })
                                .cloned()
                                .unwrap_or(Value::Null);
                            self.stack.push(field_val);
                        }
                        Value::Array(arr) => {
                            let field_val = arr.get(*idx).cloned().unwrap_or(Value::Null);
                            self.stack.push(field_val);
                        }
                        _ => self.stack.push(Value::Null),
                    }
                }
                Instruction::IndexArray => {
                    let index = self.pop()?;
                    let array = self.pop()?;
                    match (array, index) {
                        (Value::Array(arr), Value::Int(i)) => {
                            let idx = if i < 0 { (arr.len() as i64 + i) as usize } else { i as usize };
                            self.stack.push(arr.get(idx).cloned().unwrap_or(Value::Null));
                        }
                        _ => return Err(CodegenError::TypeError("index requires array and int".into())),
                    }
                }
                Instruction::SliceArray => {
                    let end = self.pop()?;
                    let start = self.pop()?;
                    let array = self.pop()?;
                    match array {
                        Value::Array(arr) => {
                            let s = match start { Value::Int(n) if n >= 0 => n as usize, _ => 0 };
                            let e = match end { Value::Int(n) if n >= 0 => n as usize, _ => arr.len() };
                            let e = e.min(arr.len());
                            let s = s.min(e);
                            self.stack.push(Value::Array(arr[s..e].to_vec()));
                        }
                        _ => return Err(CodegenError::TypeError("slice requires array".into())),
                    }
                }

                // ── Constructors ───────────────────
                Instruction::MakeConstructor(name, count) => {
                    let count = *count;
                    let mut fields = Vec::with_capacity(count);
                    for _ in 0..count {
                        fields.push(self.pop()?);
                    }
                    fields.reverse();
                    self.stack.push(Value::Constructor {
                        name: name.clone(),
                        fields,
                    });
                }

                // ── Closures ───────────────────────
                Instruction::MakeClosure(func_idx, captures) => {
                    let mut env: HashMap<String, Value> = HashMap::new();
                    for name in captures {
                        if let Some(val) = self.variables.get(name)
                            .or_else(|| old_vars.get(name))
                            .or_else(|| self.globals.get(name))
                        {
                            env.insert(name.clone(), val.clone());
                        }
                    }
                    let mut closure_map = HashMap::new();
                    closure_map.insert("__closure_func_idx".to_string(), Value::Int(*func_idx as i64));
                    for (k, v) in env {
                        closure_map.insert(format!("__capture_{k}"), v);
                    }
                    self.stack.push(Value::Object(closure_map));
                }
                Instruction::CallClosure(arg_count) => {
                    let arg_count = *arg_count;
                    let mut call_args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        call_args.push(self.pop()?);
                    }
                    call_args.reverse();
                    let closure = self.pop()?;
                    match closure {
                        Value::Object(map) => {
                            let func_idx = map.get("__closure_func_idx")
                                .and_then(|v| v.as_int())
                                .ok_or_else(|| CodegenError::TypeError("not a closure".into()))? as usize;
                            let callee = program.functions.get(func_idx)
                                .ok_or_else(|| CodegenError::TypeError("invalid closure function index".into()))?
                                .clone();
                            // Inject captured variables
                            let saved = std::mem::take(&mut self.variables);
                            for (k, v) in &map {
                                if let Some(name) = k.strip_prefix("__capture_") {
                                    self.variables.insert(name.to_string(), v.clone());
                                }
                            }
                            let result = self.run_function(callee, call_args, program)?;
                            self.variables = saved;
                            self.stack.push(result);
                        }
                        _ => return Err(CodegenError::TypeError("not a closure".into())),
                    }
                }

                // ── Debugging ───────────────────────
                Instruction::Print => {
                    if let Some(val) = self.stack.last() {
                        eprintln!("[print] {val:?}");
                    }
                }

                Instruction::Panic(msg) => {
                    return Err(CodegenError::RuntimePanic(msg.clone()));
                }

                // ── No-op ───────────────────────────
                Instruction::Nop => {}
            }

            pc += 1;
        }

        self.variables = old_vars;
        self.stack.pop().ok_or(CodegenError::StackUnderflow)
    }

    /// Like run_function but uses globals as the variable base and persists new bindings back.
    fn run_function_persistent(
        &mut self,
        func: Function,
        program: &Program,
    ) -> Result<Value, CodegenError> {
        // Use globals as the starting variable environment
        self.variables = self.globals.clone();

        let mut pc = 0;
        while pc < func.instructions.len() {
            match &func.instructions[pc] {
                Instruction::PushInt(n) => self.stack.push(Value::Int(*n)),
                Instruction::PushFloat(f) => self.stack.push(Value::Float(*f)),
                Instruction::PushBool(b) => self.stack.push(Value::Bool(*b)),
                Instruction::PushString(s) => self.stack.push(Value::String(s.clone())),
                Instruction::PushNull => self.stack.push(Value::Null),
                Instruction::LoadVar(name) => {
                    let val = self
                        .variables
                        .get(name)
                        .ok_or_else(|| CodegenError::UndefinedVariable(name.clone()))?
                        .clone();
                    self.stack.push(val);
                }
                Instruction::StoreVar(name) => {
                    let val = self.pop()?;
                    self.variables.insert(name.clone(), val);
                }
                Instruction::Add => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(arith_add(a, b)?);
                }
                Instruction::Sub => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(arith_sub(a, b)?);
                }
                Instruction::Mul => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(arith_mul(a, b)?);
                }
                Instruction::Div => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(arith_div(a, b)?);
                }
                Instruction::Mod => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(arith_mod(a, b)?);
                }
                Instruction::Concat => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    match (a, b) {
                        (Value::String(x), Value::String(y)) => {
                            self.stack.push(Value::String(format!("{x}{y}")));
                        }
                        (Value::Array(mut x), Value::Array(y)) => {
                            x.extend(y);
                            self.stack.push(Value::Array(x));
                        }
                        _ => {
                            return Err(CodegenError::TypeError(
                                "++ requires string or array operands".into(),
                            ))
                        }
                    }
                }
                Instruction::Neg => {
                    let a = self.pop()?;
                    let result = match a {
                        Value::Int(x) => Value::Int(-x),
                        Value::Float(x) => Value::Float(-x),
                        _ => {
                            return Err(CodegenError::TypeError(
                                "negation requires numeric operand".into(),
                            ))
                        }
                    };
                    self.stack.push(result);
                }
                Instruction::Eq => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(Value::Bool(values_eq(&a, &b)));
                }
                Instruction::Neq => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(Value::Bool(!values_eq(&a, &b)));
                }
                Instruction::Lt => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(Value::Bool(compare_values(&a, &b)? < 0));
                }
                Instruction::Gt => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(Value::Bool(compare_values(&a, &b)? > 0));
                }
                Instruction::Leq => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(Value::Bool(compare_values(&a, &b)? <= 0));
                }
                Instruction::Geq => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(Value::Bool(compare_values(&a, &b)? >= 0));
                }
                Instruction::And => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    match (a, b) {
                        (Value::Bool(x), Value::Bool(y)) => {
                            self.stack.push(Value::Bool(x && y));
                        }
                        _ => {
                            return Err(CodegenError::TypeError(
                                "logical AND requires bool operands".into(),
                            ))
                        }
                    }
                }
                Instruction::Or => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    match (a, b) {
                        (Value::Bool(x), Value::Bool(y)) => {
                            self.stack.push(Value::Bool(x || y));
                        }
                        _ => {
                            return Err(CodegenError::TypeError(
                                "logical OR requires bool operands".into(),
                            ))
                        }
                    }
                }
                Instruction::Not => {
                    let a = self.pop()?;
                    match a {
                        Value::Bool(x) => self.stack.push(Value::Bool(!x)),
                        _ => {
                            return Err(CodegenError::TypeError(
                                "logical NOT requires bool operand".into(),
                            ))
                        }
                    }
                }
                Instruction::Dup => {
                    let val = self
                        .stack
                        .last()
                        .ok_or(CodegenError::StackUnderflow)?
                        .clone();
                    self.stack.push(val);
                }
                Instruction::Pop => {
                    self.pop()?;
                }
                Instruction::Swap => {
                    let len = self.stack.len();
                    if len < 2 {
                        return Err(CodegenError::StackUnderflow);
                    }
                    self.stack.swap(len - 1, len - 2);
                }
                Instruction::Jump(target) => {
                    pc = *target;
                    continue;
                }
                Instruction::JumpIfFalse(target) => {
                    let val = self.pop()?;
                    match val {
                        Value::Bool(false) => {
                            pc = *target;
                            continue;
                        }
                        Value::Bool(true) => {}
                        _ => {
                            return Err(CodegenError::TypeError(
                                "conditional requires bool value".into(),
                            ))
                        }
                    }
                }
                Instruction::Call(name, arg_count) => {
                    let name = name.clone();
                    let arg_count = *arg_count;
                    let mut call_args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        call_args.push(self.pop()?);
                    }
                    call_args.reverse();
                    if let Some(result) = self.try_hof_builtin(&name, &call_args, program)? {
                        self.stack.push(result);
                    } else if self.builtins.contains_key(&name) {
                        let result = (self.builtins[&name])(call_args)?;
                        self.stack.push(result);
                    } else if let Some(callee) =
                        program.functions.iter().find(|f| f.name == name)
                            .or_else(|| self.persistent_functions.iter().find(|f| f.name == name))
                    {
                        let callee = callee.clone();
                        let result = self.run_function(callee, call_args, program)?;
                        self.stack.push(result);
                    } else if let Some(closure_val) = self.variables.get(&name)
                        .or_else(|| self.globals.get(&name))
                        .cloned()
                    {
                        let result = self.call_closure_value(closure_val, call_args, program)?;
                        self.stack.push(result);
                    } else {
                        return Err(CodegenError::UndefinedVariable(name));
                    }
                }
                Instruction::Return => break,
                Instruction::GetField(name) => {
                    let val = self.pop()?;
                    match val {
                        Value::Object(map) => {
                            let field_val = map
                                .get(name)
                                .ok_or_else(|| {
                                    CodegenError::TypeError(format!(
                                        "no field '{name}' in record"
                                    ))
                                })?
                                .clone();
                            self.stack.push(field_val);
                        }
                        _ => {
                            return Err(CodegenError::TypeError(
                                "field access on non-record".into(),
                            ))
                        }
                    }
                }
                Instruction::MakeArray(count) => {
                    let count = *count;
                    let mut elements = Vec::with_capacity(count);
                    for _ in 0..count {
                        elements.push(self.pop()?);
                    }
                    elements.reverse();
                    self.stack.push(Value::Array(elements));
                }
                Instruction::MakeRecord(fields) => {
                    let fields = fields.clone();
                    let mut values = Vec::with_capacity(fields.len());
                    for _ in 0..fields.len() {
                        values.push(self.pop()?);
                    }
                    values.reverse();
                    let map: HashMap<String, Value> =
                        fields.into_iter().zip(values).collect();
                    self.stack.push(Value::Object(map));
                }
                // ── Pattern matching ────────────────
                Instruction::TestInt(n) => {
                    let val = self.stack.last().ok_or(CodegenError::StackUnderflow)?;
                    let matches = matches!(val, Value::Int(v) if v == n);
                    self.stack.push(Value::Bool(matches));
                }
                Instruction::TestString(s) => {
                    let val = self.stack.last().ok_or(CodegenError::StackUnderflow)?;
                    let matches = matches!(val, Value::String(v) if v == s);
                    self.stack.push(Value::Bool(matches));
                }
                Instruction::TestBool(b) => {
                    let val = self.stack.last().ok_or(CodegenError::StackUnderflow)?;
                    let matches = matches!(val, Value::Bool(v) if v == b);
                    self.stack.push(Value::Bool(matches));
                }
                Instruction::TestConstructor(name) => {
                    let val = self.stack.last().ok_or(CodegenError::StackUnderflow)?;
                    let matches = match val {
                        Value::Constructor { name: n, .. } => n == name,
                        Value::Object(map) => map.get("__constructor").map_or(false, |v| {
                            matches!(v, Value::String(n) if n == name)
                        }),
                        _ => false,
                    };
                    self.stack.push(Value::Bool(matches));
                }
                Instruction::ExtractField(idx) => {
                    let val = self.pop()?;
                    match val {
                        Value::Constructor { fields, .. } => {
                            self.stack.push(fields.get(*idx).cloned().unwrap_or(Value::Null));
                        }
                        Value::Object(map) => {
                            let field_val = map
                                .get(&format!("__{idx}"))
                                .or_else(|| {
                                    let mut keys: Vec<_> = map.keys()
                                        .filter(|k| !k.starts_with("__"))
                                        .collect();
                                    keys.sort();
                                    keys.get(*idx).and_then(|k| map.get(*k))
                                })
                                .cloned()
                                .unwrap_or(Value::Null);
                            self.stack.push(field_val);
                        }
                        Value::Array(arr) => {
                            let field_val = arr.get(*idx).cloned().unwrap_or(Value::Null);
                            self.stack.push(field_val);
                        }
                        _ => self.stack.push(Value::Null),
                    }
                }
                Instruction::IndexArray => {
                    let index = self.pop()?;
                    let array = self.pop()?;
                    match (array, index) {
                        (Value::Array(arr), Value::Int(i)) => {
                            let idx = if i < 0 { (arr.len() as i64 + i) as usize } else { i as usize };
                            self.stack.push(arr.get(idx).cloned().unwrap_or(Value::Null));
                        }
                        _ => return Err(CodegenError::TypeError("index requires array and int".into())),
                    }
                }
                Instruction::SliceArray => {
                    let end = self.pop()?;
                    let start = self.pop()?;
                    let array = self.pop()?;
                    match array {
                        Value::Array(arr) => {
                            let s = match start { Value::Int(n) if n >= 0 => n as usize, _ => 0 };
                            let e = match end { Value::Int(n) if n >= 0 => n as usize, _ => arr.len() };
                            let e = e.min(arr.len());
                            let s = s.min(e);
                            self.stack.push(Value::Array(arr[s..e].to_vec()));
                        }
                        _ => return Err(CodegenError::TypeError("slice requires array".into())),
                    }
                }

                // ── Constructors ───────────────────
                Instruction::MakeConstructor(name, count) => {
                    let count = *count;
                    let mut fields = Vec::with_capacity(count);
                    for _ in 0..count {
                        fields.push(self.pop()?);
                    }
                    fields.reverse();
                    self.stack.push(Value::Constructor {
                        name: name.clone(),
                        fields,
                    });
                }

                // ── Closures ───────────────────────
                Instruction::MakeClosure(func_idx, captures) => {
                    let mut env: HashMap<String, Value> = HashMap::new();
                    for name in captures {
                        if let Some(val) = self.variables.get(name)
                            .or_else(|| self.globals.get(name))
                        {
                            env.insert(name.clone(), val.clone());
                        }
                    }
                    let mut closure_map = HashMap::new();
                    closure_map.insert("__closure_func_idx".to_string(), Value::Int(*func_idx as i64));
                    for (k, v) in env {
                        closure_map.insert(format!("__capture_{k}"), v);
                    }
                    self.stack.push(Value::Object(closure_map));
                }
                Instruction::CallClosure(arg_count) => {
                    let arg_count = *arg_count;
                    let mut call_args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        call_args.push(self.pop()?);
                    }
                    call_args.reverse();
                    let closure = self.pop()?;
                    match closure {
                        Value::Object(map) => {
                            let func_idx = map.get("__closure_func_idx")
                                .and_then(|v| v.as_int())
                                .ok_or_else(|| CodegenError::TypeError("not a closure".into()))? as usize;
                            let callee = program.functions.get(func_idx)
                                .ok_or_else(|| CodegenError::TypeError("invalid closure function index".into()))?
                                .clone();
                            let saved = std::mem::take(&mut self.variables);
                            for (k, v) in &map {
                                if let Some(name) = k.strip_prefix("__capture_") {
                                    self.variables.insert(name.to_string(), v.clone());
                                }
                            }
                            let result = self.run_function(callee, call_args, program)?;
                            self.variables = saved;
                            self.stack.push(result);
                        }
                        _ => return Err(CodegenError::TypeError("not a closure".into())),
                    }
                }

                Instruction::Print => {
                    if let Some(val) = self.stack.last() {
                        eprintln!("[print] {val:?}");
                    }
                }
                Instruction::Panic(msg) => {
                    return Err(CodegenError::RuntimePanic(msg.clone()));
                }
                Instruction::Nop => {}
            }
            pc += 1;
        }

        // Persist all variables back to globals
        self.globals = std::mem::take(&mut self.variables);
        self.stack.pop().ok_or(CodegenError::StackUnderflow)
    }
}

// ── Arithmetic helpers ─────────────────────────────

fn arith_add(a: Value, b: Value) -> Result<Value, CodegenError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x + y)),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x + y)),
        (Value::Int(x), Value::Float(y)) => Ok(Value::Float(x as f64 + y)),
        (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x + y as f64)),
        (Value::String(x), Value::String(y)) => Ok(Value::String(format!("{x}{y}"))),
        _ => Err(CodegenError::TypeError("invalid operands for +".into())),
    }
}

fn arith_sub(a: Value, b: Value) -> Result<Value, CodegenError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x - y)),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x - y)),
        (Value::Int(x), Value::Float(y)) => Ok(Value::Float(x as f64 - y)),
        (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x - y as f64)),
        _ => Err(CodegenError::TypeError("invalid operands for -".into())),
    }
}

fn arith_mul(a: Value, b: Value) -> Result<Value, CodegenError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x * y)),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x * y)),
        (Value::Int(x), Value::Float(y)) => Ok(Value::Float(x as f64 * y)),
        (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x * y as f64)),
        _ => Err(CodegenError::TypeError("invalid operands for *".into())),
    }
}

fn arith_div(a: Value, b: Value) -> Result<Value, CodegenError> {
    match (a, b) {
        (Value::Int(_), Value::Int(0)) => Err(CodegenError::DivisionByZero),
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x / y)),
        (Value::Float(_), Value::Float(y)) if y == 0.0 => Err(CodegenError::DivisionByZero),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x / y)),
        (Value::Int(_), Value::Float(y)) if y == 0.0 => Err(CodegenError::DivisionByZero),
        (Value::Int(x), Value::Float(y)) => Ok(Value::Float(x as f64 / y)),
        (Value::Float(_), Value::Int(0)) => Err(CodegenError::DivisionByZero),
        (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x / y as f64)),
        _ => Err(CodegenError::TypeError("invalid operands for /".into())),
    }
}

fn arith_mod(a: Value, b: Value) -> Result<Value, CodegenError> {
    match (a, b) {
        (Value::Int(_), Value::Int(0)) => Err(CodegenError::DivisionByZero),
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x % y)),
        _ => Err(CodegenError::TypeError("invalid operands for %".into())),
    }
}

fn values_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Int(x), Value::Float(y)) => (*x as f64) == *y,
        (Value::Float(x), Value::Int(y)) => *x == (*y as f64),
        (Value::String(x), Value::String(y)) => x == y,
        (
            Value::Constructor { name: n1, fields: f1 },
            Value::Constructor { name: n2, fields: f2 },
        ) => n1 == n2 && f1.len() == f2.len() && f1.iter().zip(f2).all(|(a, b)| values_eq(a, b)),
        _ => false,
    }
}

/// Returns negative if a < b, zero if a == b, positive if a > b.
fn compare_values(a: &Value, b: &Value) -> Result<i8, CodegenError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y) as i8),
        (Value::Float(x), Value::Float(y)) => float_cmp(*x, *y),
        (Value::Int(x), Value::Float(y)) => float_cmp(*x as f64, *y),
        (Value::Float(x), Value::Int(y)) => float_cmp(*x, *y as f64),
        (Value::String(x), Value::String(y)) => Ok(x.cmp(y) as i8),
        _ => Err(CodegenError::TypeError(
            "incomparable types".into(),
        )),
    }
}

fn float_cmp(a: f64, b: f64) -> Result<i8, CodegenError> {
    a.partial_cmp(&b)
        .map(|o| o as i8)
        .ok_or_else(|| CodegenError::TypeError("NaN comparison".into()))
}

// ── Helper: compile + execute in one shot ──────────

/// Convenience: compile an AST expression and immediately execute it.
pub fn eval_expr(expr: &lattice_parser::ast::Expr) -> Result<Value, CodegenError> {
    let mut compiler = crate::compiler::Compiler::new();
    let program = compiler.compile_expression(expr)?;
    let mut interp = Interpreter::new();
    interp.register_stdlib();
    interp.execute(&program)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_parser::ast::{BinOp, Expr, Spanned, UnaryOp};

    fn s(expr: Expr) -> Spanned<Expr> {
        Spanned::dummy(expr)
    }

    fn int(n: i64) -> Spanned<Expr> {
        s(Expr::IntLit(n))
    }

    fn float(f: f64) -> Spanned<Expr> {
        s(Expr::FloatLit(f))
    }

    fn bool_lit(b: bool) -> Spanned<Expr> {
        s(Expr::BoolLit(b))
    }

    fn binop(left: Spanned<Expr>, op: BinOp, right: Spanned<Expr>) -> Expr {
        Expr::BinOp {
            left: Box::new(left),
            op,
            right: Box::new(right),
        }
    }

    // ── Arithmetic ──────────────────────────

    #[test]
    fn arithmetic_2_plus_3_times_4() {
        // 2 + (3 * 4) = 14
        let expr = binop(int(2), BinOp::Add, s(binop(int(3), BinOp::Mul, int(4))));
        assert_eq!(eval_expr(&expr).unwrap(), Value::Int(14));
    }

    #[test]
    fn arithmetic_subtraction() {
        let expr = binop(int(10), BinOp::Sub, int(3));
        assert_eq!(eval_expr(&expr).unwrap(), Value::Int(7));
    }

    #[test]
    fn arithmetic_float() {
        let expr = binop(float(1.5), BinOp::Add, float(2.5));
        assert_eq!(eval_expr(&expr).unwrap(), Value::Float(4.0));
    }

    #[test]
    fn arithmetic_mixed_int_float() {
        let expr = binop(int(2), BinOp::Mul, float(3.0));
        assert_eq!(eval_expr(&expr).unwrap(), Value::Float(6.0));
    }

    #[test]
    fn modulo() {
        let expr = binop(int(17), BinOp::Mod, int(5));
        assert_eq!(eval_expr(&expr).unwrap(), Value::Int(2));
    }

    // ── Boolean logic ───────────────────────

    #[test]
    fn boolean_and_false() {
        let expr = binop(bool_lit(true), BinOp::And, bool_lit(false));
        assert_eq!(eval_expr(&expr).unwrap(), Value::Bool(false));
    }

    #[test]
    fn boolean_or() {
        let expr = binop(bool_lit(false), BinOp::Or, bool_lit(true));
        assert_eq!(eval_expr(&expr).unwrap(), Value::Bool(true));
    }

    #[test]
    fn boolean_not() {
        let expr = Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(bool_lit(true)),
        };
        assert_eq!(eval_expr(&expr).unwrap(), Value::Bool(false));
    }

    // ── Variables ───────────────────────────

    #[test]
    fn let_binding_and_use() {
        // Block: [let x = 5, x + 3]
        let expr = Expr::Block(vec![
            s(Expr::Let {
                name: "x".into(),
                type_ann: None,
                value: Box::new(int(5)),
            }),
            s(binop(s(Expr::Ident("x".into())), BinOp::Add, int(3))),
        ]);
        assert_eq!(eval_expr(&expr).unwrap(), Value::Int(8));
    }

    // ── Comparison ──────────────────────────

    #[test]
    fn comparison_gt() {
        let expr = binop(int(10), BinOp::Gt, int(5));
        assert_eq!(eval_expr(&expr).unwrap(), Value::Bool(true));
    }

    #[test]
    fn comparison_eq() {
        let expr = binop(int(3), BinOp::Eq, int(3));
        assert_eq!(eval_expr(&expr).unwrap(), Value::Bool(true));
    }

    #[test]
    fn comparison_leq() {
        let expr = binop(int(5), BinOp::Leq, int(5));
        assert_eq!(eval_expr(&expr).unwrap(), Value::Bool(true));
    }

    #[test]
    fn comparison_lt_false() {
        let expr = binop(int(10), BinOp::Lt, int(5));
        assert_eq!(eval_expr(&expr).unwrap(), Value::Bool(false));
    }

    // ── If expression ───────────────────────

    #[test]
    fn if_true_branch() {
        let expr = Expr::If {
            cond: Box::new(bool_lit(true)),
            then_: Box::new(int(42)),
            else_: Some(Box::new(int(0))),
        };
        assert_eq!(eval_expr(&expr).unwrap(), Value::Int(42));
    }

    #[test]
    fn if_false_branch() {
        let expr = Expr::If {
            cond: Box::new(bool_lit(false)),
            then_: Box::new(int(42)),
            else_: Some(Box::new(int(0))),
        };
        assert_eq!(eval_expr(&expr).unwrap(), Value::Int(0));
    }

    // ── Function call with builtins ─────────

    #[test]
    fn builtin_function_call() {
        let mut compiler = crate::compiler::Compiler::new();
        let expr = Expr::Call {
            func: Box::new(s(Expr::Ident("double".into()))),
            args: vec![int(7)],
        };
        let program = compiler.compile_expression(&expr).unwrap();

        let mut interp = Interpreter::new();
        interp.register_builtin("double".into(), |args| match &args[0] {
            Value::Int(n) => Ok(Value::Int(n * 2)),
            _ => Err(CodegenError::TypeError("expected int".into())),
        });

        assert_eq!(interp.execute(&program).unwrap(), Value::Int(14));
    }

    // ── Pipeline ────────────────────────────

    #[test]
    fn pipeline_to_builtin() {
        let mut compiler = crate::compiler::Compiler::new();
        let expr = Expr::Pipeline {
            left: Box::new(int(5)),
            right: Box::new(s(Expr::Ident("double".into()))),
        };
        let program = compiler.compile_expression(&expr).unwrap();

        let mut interp = Interpreter::new();
        interp.register_builtin("double".into(), |args| match &args[0] {
            Value::Int(n) => Ok(Value::Int(n * 2)),
            _ => Err(CodegenError::TypeError("expected int".into())),
        });

        assert_eq!(interp.execute(&program).unwrap(), Value::Int(10));
    }

    // ── Arrays ──────────────────────────────

    #[test]
    fn array_construction() {
        let expr = Expr::Array(vec![int(1), int(2), int(3)]);
        assert_eq!(
            eval_expr(&expr).unwrap(),
            Value::Array(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
        );
    }

    // ── Records ─────────────────────────────

    #[test]
    fn record_construction_and_field_access() {
        let record = Expr::Record(vec![
            ("x".into(), int(10)),
            ("y".into(), int(20)),
        ]);
        let expr = Expr::Field {
            expr: Box::new(s(record)),
            name: "x".into(),
        };
        assert_eq!(eval_expr(&expr).unwrap(), Value::Int(10));
    }

    // ── Negation ────────────────────────────

    #[test]
    fn unary_negation() {
        let expr = Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(int(7)),
        };
        assert_eq!(eval_expr(&expr).unwrap(), Value::Int(-7));
    }

    // ── String concatenation ────────────────

    #[test]
    fn string_concat() {
        let expr = binop(
            s(Expr::StringLit("hello ".into())),
            BinOp::Add,
            s(Expr::StringLit("world".into())),
        );
        assert_eq!(
            eval_expr(&expr).unwrap(),
            Value::String("hello world".into())
        );
    }

    // ── String concatenation (++) ────────────

    #[test]
    fn string_concat_operator() {
        let expr = binop(
            s(Expr::StringLit("hello ".into())),
            BinOp::Concat,
            s(Expr::StringLit("world".into())),
        );
        assert_eq!(
            eval_expr(&expr).unwrap(),
            Value::String("hello world".into())
        );
    }

    #[test]
    fn array_concat_operator() {
        let expr = Expr::BinOp {
            left: Box::new(s(Expr::Array(vec![int(1), int(2)]))),
            op: BinOp::Concat,
            right: Box::new(s(Expr::Array(vec![int(3), int(4)]))),
        };
        assert_eq!(
            eval_expr(&expr).unwrap(),
            Value::Array(vec![Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4)])
        );
    }

    // ── Match expression ────────────────────

    #[test]
    fn match_literal_int() {
        use lattice_parser::ast::{MatchArm, Pattern};
        let expr = Expr::Match {
            expr: Box::new(int(42)),
            arms: vec![
                MatchArm {
                    pattern: Spanned::dummy(Pattern::Literal(int(1))),
                    guard: None,
                    body: s(Expr::StringLit("one".into())),
                },
                MatchArm {
                    pattern: Spanned::dummy(Pattern::Literal(int(42))),
                    guard: None,
                    body: s(Expr::StringLit("forty-two".into())),
                },
            ],
        };
        assert_eq!(
            eval_expr(&expr).unwrap(),
            Value::String("forty-two".into())
        );
    }

    #[test]
    fn match_wildcard() {
        use lattice_parser::ast::{MatchArm, Pattern};
        let expr = Expr::Match {
            expr: Box::new(int(99)),
            arms: vec![
                MatchArm {
                    pattern: Spanned::dummy(Pattern::Literal(int(1))),
                    guard: None,
                    body: s(Expr::StringLit("one".into())),
                },
                MatchArm {
                    pattern: Spanned::dummy(Pattern::Wildcard),
                    guard: None,
                    body: s(Expr::StringLit("other".into())),
                },
            ],
        };
        assert_eq!(
            eval_expr(&expr).unwrap(),
            Value::String("other".into())
        );
    }

    #[test]
    fn match_ident_binding() {
        use lattice_parser::ast::{MatchArm, Pattern};
        let expr = Expr::Match {
            expr: Box::new(int(7)),
            arms: vec![MatchArm {
                pattern: Spanned::dummy(Pattern::Ident("x".into())),
                guard: None,
                body: s(binop(s(Expr::Ident("x".into())), BinOp::Mul, int(2))),
            }],
        };
        assert_eq!(eval_expr(&expr).unwrap(), Value::Int(14));
    }

    // ── Lambda/closure ─────────────────────

    #[test]
    fn lambda_basic() {
        use lattice_parser::ast::Param;
        // let double = fn(x) -> x * 2; double(5)
        let expr = Expr::Block(vec![
            s(Expr::Let {
                name: "double".into(),
                type_ann: None,
                value: Box::new(s(Expr::Lambda {
                    params: vec![Param {
                        name: "x".into(),
                        type_expr: Spanned::dummy(lattice_parser::ast::TypeExpr::Named("Int".into())),
                    }],
                    body: Box::new(s(binop(
                        s(Expr::Ident("x".into())),
                        BinOp::Mul,
                        int(2),
                    ))),
                })),
            }),
            s(Expr::Call {
                func: Box::new(s(Expr::Ident("double".into()))),
                args: vec![int(5)],
            }),
        ]);
        assert_eq!(eval_expr(&expr).unwrap(), Value::Int(10));
    }

    // ── Array operations ────────────────────

    #[test]
    fn array_index() {
        let expr = Expr::Index {
            expr: Box::new(s(Expr::Array(vec![int(10), int(20), int(30)]))),
            index: Box::new(int(1)),
        };
        assert_eq!(eval_expr(&expr).unwrap(), Value::Int(20));
    }

    #[test]
    fn array_index_negative() {
        let expr = Expr::Index {
            expr: Box::new(s(Expr::Array(vec![int(10), int(20), int(30)]))),
            index: Box::new(int(-1)),
        };
        assert_eq!(eval_expr(&expr).unwrap(), Value::Int(30));
    }

    #[test]
    fn array_slice() {
        let expr = Expr::Slice {
            expr: Box::new(s(Expr::Array(vec![int(1), int(2), int(3), int(4)]))),
            start: Some(Box::new(int(1))),
            end: Some(Box::new(int(3))),
        };
        assert_eq!(
            eval_expr(&expr).unwrap(),
            Value::Array(vec![Value::Int(2), Value::Int(3)])
        );
    }

    #[test]
    fn builtin_length() {
        let expr = Expr::Call {
            func: Box::new(s(Expr::Ident("length".into()))),
            args: vec![s(Expr::Array(vec![int(1), int(2), int(3)]))],
        };
        assert_eq!(eval_expr(&expr).unwrap(), Value::Int(3));
    }

    #[test]
    fn builtin_head_tail() {
        let arr = Expr::Array(vec![int(10), int(20), int(30)]);
        let head_expr = Expr::Call {
            func: Box::new(s(Expr::Ident("head".into()))),
            args: vec![s(arr.clone())],
        };
        assert_eq!(eval_expr(&head_expr).unwrap(), Value::Int(10));

        let tail_expr = Expr::Call {
            func: Box::new(s(Expr::Ident("tail".into()))),
            args: vec![s(arr)],
        };
        assert_eq!(
            eval_expr(&tail_expr).unwrap(),
            Value::Array(vec![Value::Int(20), Value::Int(30)])
        );
    }

    #[test]
    fn builtin_reverse() {
        let expr = Expr::Call {
            func: Box::new(s(Expr::Ident("reverse".into()))),
            args: vec![s(Expr::Array(vec![int(1), int(2), int(3)]))],
        };
        assert_eq!(
            eval_expr(&expr).unwrap(),
            Value::Array(vec![Value::Int(3), Value::Int(2), Value::Int(1)])
        );
    }

    // ── Higher-order array ops ──────────────

    #[test]
    fn map_with_lambda() {
        use lattice_parser::ast::Param;
        // map([1, 2, 3], fn(x) -> x * 2) => [2, 4, 6]
        let expr = Expr::Call {
            func: Box::new(s(Expr::Ident("map".into()))),
            args: vec![
                s(Expr::Array(vec![int(1), int(2), int(3)])),
                s(Expr::Lambda {
                    params: vec![Param {
                        name: "x".into(),
                        type_expr: Spanned::dummy(lattice_parser::ast::TypeExpr::Named("Int".into())),
                    }],
                    body: Box::new(s(binop(
                        s(Expr::Ident("x".into())),
                        BinOp::Mul,
                        int(2),
                    ))),
                }),
            ],
        };
        assert_eq!(
            eval_expr(&expr).unwrap(),
            Value::Array(vec![Value::Int(2), Value::Int(4), Value::Int(6)])
        );
    }

    #[test]
    fn filter_with_lambda() {
        use lattice_parser::ast::Param;
        // filter([1, 2, 3, 4, 5], fn(x) -> x > 2) => [3, 4, 5]
        let expr = Expr::Call {
            func: Box::new(s(Expr::Ident("filter".into()))),
            args: vec![
                s(Expr::Array(vec![int(1), int(2), int(3), int(4), int(5)])),
                s(Expr::Lambda {
                    params: vec![Param {
                        name: "x".into(),
                        type_expr: Spanned::dummy(lattice_parser::ast::TypeExpr::Named("Int".into())),
                    }],
                    body: Box::new(s(binop(
                        s(Expr::Ident("x".into())),
                        BinOp::Gt,
                        int(2),
                    ))),
                }),
            ],
        };
        assert_eq!(
            eval_expr(&expr).unwrap(),
            Value::Array(vec![Value::Int(3), Value::Int(4), Value::Int(5)])
        );
    }

    #[test]
    fn fold_sum() {
        use lattice_parser::ast::Param;
        // fold([1, 2, 3, 4], 0, fn(acc, x) -> acc + x) => 10
        let expr = Expr::Call {
            func: Box::new(s(Expr::Ident("fold".into()))),
            args: vec![
                s(Expr::Array(vec![int(1), int(2), int(3), int(4)])),
                int(0),
                s(Expr::Lambda {
                    params: vec![
                        Param {
                            name: "acc".into(),
                            type_expr: Spanned::dummy(lattice_parser::ast::TypeExpr::Named("Int".into())),
                        },
                        Param {
                            name: "x".into(),
                            type_expr: Spanned::dummy(lattice_parser::ast::TypeExpr::Named("Int".into())),
                        },
                    ],
                    body: Box::new(s(binop(
                        s(Expr::Ident("acc".into())),
                        BinOp::Add,
                        s(Expr::Ident("x".into())),
                    ))),
                }),
            ],
        };
        assert_eq!(eval_expr(&expr).unwrap(), Value::Int(10));
    }

    #[test]
    fn lambda_captures_variable() {
        use lattice_parser::ast::Param;
        // let multiplier = 3; let f = fn(x) -> x * multiplier; f(10) => 30
        let expr = Expr::Block(vec![
            s(Expr::Let {
                name: "multiplier".into(),
                type_ann: None,
                value: Box::new(int(3)),
            }),
            s(Expr::Let {
                name: "f".into(),
                type_ann: None,
                value: Box::new(s(Expr::Lambda {
                    params: vec![Param {
                        name: "x".into(),
                        type_expr: Spanned::dummy(lattice_parser::ast::TypeExpr::Named("Int".into())),
                    }],
                    body: Box::new(s(binop(
                        s(Expr::Ident("x".into())),
                        BinOp::Mul,
                        s(Expr::Ident("multiplier".into())),
                    ))),
                })),
            }),
            s(Expr::Call {
                func: Box::new(s(Expr::Ident("f".into()))),
                args: vec![int(10)],
            }),
        ]);
        assert_eq!(eval_expr(&expr).unwrap(), Value::Int(30));
    }

    // ── Error cases ─────────────────────────

    #[test]
    fn stack_underflow_error() {
        let program = Program {
            functions: vec![Function {
                name: "__main__".into(),
                params: vec![],
                instructions: vec![Instruction::Add, Instruction::Return],
            }],
            entry: 0,
        };
        let mut interp = Interpreter::new();
        let result = interp.execute(&program);
        assert!(matches!(result, Err(CodegenError::StackUnderflow)));
    }

    #[test]
    fn division_by_zero_error() {
        let expr = binop(int(10), BinOp::Div, int(0));
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(CodegenError::DivisionByZero)));
    }

    #[test]
    fn undefined_variable_error() {
        let expr = Expr::Ident("nonexistent".into());
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(CodegenError::UndefinedVariable(_))));
    }

    #[test]
    fn type_error_add_bool_int() {
        let expr = binop(bool_lit(true), BinOp::Add, int(1));
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(CodegenError::TypeError(_))));
    }

    #[test]
    fn make_constructor_no_fields() {
        let program = Program {
            functions: vec![Function {
                name: "__expr__".into(),
                params: vec![],
                instructions: vec![
                    Instruction::MakeConstructor("None".into(), 0),
                    Instruction::Return,
                ],
            }],
            entry: 0,
        };
        let mut interp = Interpreter::new();
        let result = interp.execute(&program).unwrap();
        assert_eq!(
            result,
            Value::Constructor {
                name: "None".into(),
                fields: vec![],
            }
        );
    }

    #[test]
    fn make_constructor_with_fields() {
        let program = Program {
            functions: vec![Function {
                name: "__expr__".into(),
                params: vec![],
                instructions: vec![
                    Instruction::PushInt(42),
                    Instruction::MakeConstructor("Some".into(), 1),
                    Instruction::Return,
                ],
            }],
            entry: 0,
        };
        let mut interp = Interpreter::new();
        let result = interp.execute(&program).unwrap();
        assert_eq!(
            result,
            Value::Constructor {
                name: "Some".into(),
                fields: vec![Value::Int(42)],
            }
        );
    }

    #[test]
    fn test_constructor_and_extract_field() {
        let program = Program {
            functions: vec![Function {
                name: "__expr__".into(),
                params: vec![],
                instructions: vec![
                    // Build Some(42)
                    Instruction::PushInt(42),
                    Instruction::MakeConstructor("Some".into(), 1),
                    // Test it's a "Some"
                    Instruction::Dup,
                    Instruction::TestConstructor("Some".into()),
                    Instruction::StoreVar("is_some".into()),
                    // Extract field 0
                    Instruction::ExtractField(0),
                    Instruction::Return,
                ],
            }],
            entry: 0,
        };
        let mut interp = Interpreter::new();
        let result = interp.execute(&program).unwrap();
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn constructor_equality() {
        let program = Program {
            functions: vec![Function {
                name: "__expr__".into(),
                params: vec![],
                instructions: vec![
                    Instruction::PushInt(1),
                    Instruction::MakeConstructor("A".into(), 1),
                    Instruction::PushInt(1),
                    Instruction::MakeConstructor("A".into(), 1),
                    Instruction::Eq,
                    Instruction::Return,
                ],
            }],
            entry: 0,
        };
        let mut interp = Interpreter::new();
        assert_eq!(interp.execute(&program).unwrap(), Value::Bool(true));
    }

    #[test]
    fn constructor_inequality_different_name() {
        let program = Program {
            functions: vec![Function {
                name: "__expr__".into(),
                params: vec![],
                instructions: vec![
                    Instruction::PushInt(1),
                    Instruction::MakeConstructor("A".into(), 1),
                    Instruction::PushInt(1),
                    Instruction::MakeConstructor("B".into(), 1),
                    Instruction::Eq,
                    Instruction::Return,
                ],
            }],
            entry: 0,
        };
        let mut interp = Interpreter::new();
        assert_eq!(interp.execute(&program).unwrap(), Value::Bool(false));
    }

    // ── Relational algebra ──────────────────────────

    /// Helper: make a record expression { k1: v1, k2: v2, ... }
    fn record(fields: Vec<(&str, Expr)>) -> Expr {
        Expr::Record(
            fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), s(v)))
                .collect(),
        )
    }

    /// Helper: make a table (array of records)
    fn table(rows: Vec<Expr>) -> Expr {
        Expr::Array(rows.into_iter().map(|r| s(r)).collect())
    }

    /// Helper: field access `row.field`
    fn row_field(field: &str) -> Expr {
        Expr::Field {
            expr: Box::new(s(Expr::Ident("row".into()))),
            name: field.to_string(),
        }
    }

    #[test]
    fn rel_select_filters_rows() {
        // SELECT * FROM people WHERE age > 25
        let people = table(vec![
            record(vec![("name", Expr::StringLit("Alice".into())), ("age", Expr::IntLit(30))]),
            record(vec![("name", Expr::StringLit("Bob".into())), ("age", Expr::IntLit(20))]),
            record(vec![("name", Expr::StringLit("Carol".into())), ("age", Expr::IntLit(35))]),
        ]);
        let predicate = binop(int(25), BinOp::Lt, s(row_field("age")));
        let expr = Expr::Select {
            relation: Box::new(s(people)),
            predicate: Box::new(s(predicate)),
        };
        let result = eval_expr(&expr).unwrap();
        if let Value::Array(rows) = result {
            assert_eq!(rows.len(), 2); // Alice(30) and Carol(35)
        } else {
            panic!("expected array, got {result:?}");
        }
    }

    #[test]
    fn rel_project_selects_fields() {
        // SELECT name FROM people
        let people = table(vec![
            record(vec![("name", Expr::StringLit("Alice".into())), ("age", Expr::IntLit(30))]),
            record(vec![("name", Expr::StringLit("Bob".into())), ("age", Expr::IntLit(20))]),
        ]);
        let expr = Expr::Project {
            relation: Box::new(s(people)),
            fields: vec!["name".to_string()],
        };
        let result = eval_expr(&expr).unwrap();
        if let Value::Array(rows) = result {
            assert_eq!(rows.len(), 2);
            // Each row should only have "name" field
            for row in &rows {
                if let Value::Object(obj) = row {
                    assert_eq!(obj.len(), 1);
                    assert!(obj.contains_key("name"));
                } else {
                    panic!("expected object, got {row:?}");
                }
            }
        } else {
            panic!("expected array, got {result:?}");
        }
    }

    #[test]
    fn rel_join_cross_product_with_filter() {
        // JOIN people, departments ON row.dept == row.dept_name
        let people = table(vec![
            record(vec![("name", Expr::StringLit("Alice".into())), ("dept", Expr::StringLit("eng".into()))]),
            record(vec![("name", Expr::StringLit("Bob".into())), ("dept", Expr::StringLit("sales".into()))]),
        ]);
        let departments = table(vec![
            record(vec![("dept_name", Expr::StringLit("eng".into())), ("floor", Expr::IntLit(3))]),
            record(vec![("dept_name", Expr::StringLit("sales".into())), ("floor", Expr::IntLit(1))]),
        ]);
        let condition = binop(
            s(row_field("dept")),
            BinOp::Eq,
            s(row_field("dept_name")),
        );
        let expr = Expr::Join {
            left: Box::new(s(people)),
            right: Box::new(s(departments)),
            condition: Box::new(s(condition)),
        };
        let result = eval_expr(&expr).unwrap();
        if let Value::Array(rows) = result {
            assert_eq!(rows.len(), 2); // Alice-eng, Bob-sales
            // Each merged row has all 3 fields: name, dept, dept_name, floor
            for row in &rows {
                if let Value::Object(obj) = row {
                    assert!(obj.contains_key("name"));
                    assert!(obj.contains_key("floor"));
                } else {
                    panic!("expected object, got {row:?}");
                }
            }
        } else {
            panic!("expected array, got {result:?}");
        }
    }

    #[test]
    fn rel_group_by_without_aggregate() {
        // GROUP BY dept (no aggregate → returns grouped arrays)
        let people = table(vec![
            record(vec![("name", Expr::StringLit("Alice".into())), ("dept", Expr::StringLit("eng".into()))]),
            record(vec![("name", Expr::StringLit("Bob".into())), ("dept", Expr::StringLit("eng".into()))]),
            record(vec![("name", Expr::StringLit("Carol".into())), ("dept", Expr::StringLit("sales".into()))]),
        ]);
        let expr = Expr::GroupBy {
            relation: Box::new(s(people)),
            keys: vec!["dept".to_string()],
            aggregates: vec![],
        };
        let result = eval_expr(&expr).unwrap();
        if let Value::Array(groups) = result {
            assert_eq!(groups.len(), 2); // eng group, sales group
            // One group has 2 items, the other has 1
            let mut sizes: Vec<usize> = groups.iter().map(|g| {
                if let Value::Array(a) = g { a.len() } else { 0 }
            }).collect();
            sizes.sort();
            assert_eq!(sizes, vec![1, 2]);
        } else {
            panic!("expected array, got {result:?}");
        }
    }

    #[test]
    fn rel_select_empty_result() {
        // SELECT * FROM people WHERE age > 100 → empty
        let people = table(vec![
            record(vec![("age", Expr::IntLit(30))]),
            record(vec![("age", Expr::IntLit(20))]),
        ]);
        let predicate = binop(int(100), BinOp::Lt, s(row_field("age")));
        let expr = Expr::Select {
            relation: Box::new(s(people)),
            predicate: Box::new(s(predicate)),
        };
        let result = eval_expr(&expr).unwrap();
        assert_eq!(result, Value::Array(vec![]));
    }
}
