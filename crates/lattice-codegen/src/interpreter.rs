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

    /// Execute a compiled program, returning the final value.
    pub fn execute(&mut self, program: &Program) -> Result<Value, CodegenError> {
        let entry = program.functions[program.entry].clone();
        self.run_function(entry, vec![], program)
    }

    /// Execute a compiled program, persisting top-level variables for REPL use.
    pub fn execute_persistent(&mut self, program: &Program) -> Result<Value, CodegenError> {
        let entry = program.functions[program.entry].clone();
        self.run_function_persistent(entry, program)
    }

    /// Returns a reference to the persistent global variables.
    pub fn globals(&self) -> &HashMap<String, Value> {
        &self.globals
    }

    fn pop(&mut self) -> Result<Value, CodegenError> {
        self.stack.pop().ok_or(CodegenError::StackUnderflow)
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

                    let is_builtin = self.builtins.contains_key(&name);
                    if is_builtin {
                        let result = (self.builtins[&name])(call_args)?;
                        self.stack.push(result);
                    } else if let Some(callee) =
                        program.functions.iter().find(|f| f.name == name)
                    {
                        let callee = callee.clone();
                        let result = self.run_function(callee, call_args, program)?;
                        self.stack.push(result);
                    } else if let Some(closure_val) = self.variables.get(&name)
                        .or_else(|| old_vars.get(&name))
                        .or_else(|| self.globals.get(&name))
                        .cloned()
                    {
                        // Try calling as a closure
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
                    let is_builtin = self.builtins.contains_key(&name);
                    if is_builtin {
                        let result = (self.builtins[&name])(call_args)?;
                        self.stack.push(result);
                    } else if let Some(callee) =
                        program.functions.iter().find(|f| f.name == name)
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
}
