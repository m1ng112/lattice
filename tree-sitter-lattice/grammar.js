/// <reference types="tree-sitter-cli/dsl" />
// Tree-sitter grammar for the Lattice programming language.

const PREC = {
  PIPELINE: 1,
  OR: 2,
  AND: 3,
  COMPARE: 4,
  MEMBERSHIP: 5,
  IMPLIES: 5,
  ADD: 6,
  MULT: 7,
  UNARY: 8,
  POSTFIX: 9,
  CALL: 10,
  FIELD: 11,
  INDEX: 12,
};

module.exports = grammar({
  name: 'lattice',

  extras: $ => [
    /\s/,
    $.line_comment,
  ],

  word: $ => $.identifier,

  conflicts: $ => [
    [$._type_expr, $._expression],
    [$._type_body, $._type_expr],
    [$.refinement_type, $._expression],
    [$.block_field, $.record_literal],
    [$.block_field, $.set_literal],
  ],

  rules: {
    source_file: $ => repeat($._item),

    // ─── Top-level items ─────────────────────────────────
    _item: $ => choice(
      $.graph_declaration,
      $.type_definition,
      $.function_definition,
      $.module_declaration,
      $.model_declaration,
      $.meta_block,
      $.let_binding,
    ),

    // ─── Graph ───────────────────────────────────────────
    graph_declaration: $ => seq(
      'graph',
      field('name', $.identifier),
      '{',
      repeat(choice(
        $.version_field,
        $.target_field,
        $.solve_block,
        $.graph_member,
      )),
      '}',
    ),

    graph_member: $ => choice(
      $.node_definition,
      $.edge_declaration,
    ),

    version_field: $ => seq('version', ':', $.string_literal),

    target_field: $ => seq('target', ':', $.array_literal),

    // ─── Node ────────────────────────────────────────────
    node_definition: $ => seq(
      'node',
      field('name', $.identifier),
      '{',
      repeat($.node_body_item),
      '}',
    ),

    node_body_item: $ => choice(
      $.input_field,
      $.output_field,
      $.properties_block,
      $.semantic_block,
      $.proof_obligations_block,
      $.solve_block,
      $.pre_block,
      $.post_block,
    ),

    input_field: $ => seq('input', ':', $._type_or_expr),
    output_field: $ => seq('output', ':', $._type_or_expr),

    _type_or_expr: $ => choice($._type_expr, $._expression),

    // ─── Edge ────────────────────────────────────────────
    edge_declaration: $ => seq(
      'edge',
      field('source', $.identifier),
      '->',
      field('target', $.identifier),
      optional(seq('{', repeat($.property_pair), '}')),
    ),

    // ─── Solve / Intent ──────────────────────────────────
    solve_block: $ => seq(
      'solve',
      '{',
      repeat($.solve_item),
      '}',
    ),

    solve_item: $ => choice(
      $.goal_field,
      $.constraint_field,
      $.invariant_field,
      $.domain_field,
      $.strategy_field,
    ),

    goal_field: $ => seq('goal', ':', $._expression),
    constraint_field: $ => seq('constraint', ':', $._expression),
    invariant_field: $ => seq('invariant', ':', $._expression),
    domain_field: $ => seq('domain', ':', $._expression),
    strategy_field: $ => seq('strategy', ':', $._expression),

    // ─── Function ────────────────────────────────────────
    function_definition: $ => seq(
      'function',
      field('name', $.identifier),
      optional($.type_parameters),
      '(',
      optional($.parameter_list),
      ')',
      optional(seq('->', field('return_type', $._type_expr))),
      '{',
      repeat($.function_body_item),
      '}',
    ),

    type_parameters: $ => seq(
      '<',
      commaSep1($.type_parameter),
      '>',
    ),

    type_parameter: $ => seq(
      $.identifier,
      optional(seq(':', $._type_expr)),
    ),

    parameter_list: $ => commaSep1($.parameter),

    parameter: $ => seq(
      field('name', $.identifier),
      ':',
      field('type', $._type_expr),
    ),

    function_body_item: $ => choice(
      $.pre_block,
      $.post_block,
      $.invariant_block,
      $.synthesize_call,
      $.let_binding,
      $._expression,
    ),

    pre_block: $ => seq('pre', ':', '{', repeat($._expression), '}'),
    post_block: $ => seq('post', ':', '{', repeat($._expression), '}'),
    invariant_block: $ => seq('invariant', ':', '{', repeat($._expression), '}'),

    synthesize_call: $ => seq(
      'synthesize',
      '(',
      optional(commaSep1($.property_pair)),
      ')',
    ),

    // ─── Type definitions ────────────────────────────────
    type_definition: $ => seq(
      'type',
      field('name', $.identifier),
      optional($.type_parameters),
      optional(seq('(', commaSep1($.parameter), ')')),
      '=',
      field('type', $._type_body),
    ),

    _type_body: $ => choice(
      $.sum_type,
      $.refinement_type,
      $._type_expr,
    ),

    sum_type: $ => seq(
      optional('|'),
      $.sum_variant,
      repeat1(seq('|', $.sum_variant)),
    ),

    sum_variant: $ => seq(
      $.identifier,
      optional(seq('(', commaSep1($.variant_field), ')')),
    ),

    variant_field: $ => choice(
      seq($.identifier, ':', $._type_expr),
      $._type_expr,
    ),

    refinement_type: $ => seq(
      '{',
      field('variable', $.identifier),
      choice('∈', 'in'),
      field('base_type', $._type_expr),
      '|',
      field('predicate', $._expression),
      '}',
    ),

    _type_expr: $ => choice(
      $.identifier,
      $.generic_type,
      $.function_type,
      $.refinement_type,
    ),

    generic_type: $ => prec(1, seq(
      $.identifier,
      '<',
      commaSep1(choice($._type_expr, $._expression)),
      '>',
    )),

    function_type: $ => prec.right(seq(
      $._type_expr,
      choice('→', '->'),
      $._type_expr,
    )),

    // ─── Module ──────────────────────────────────────────
    module_declaration: $ => seq(
      'module',
      field('name', $.identifier),
      '{',
      repeat($._item),
      '}',
    ),

    // ─── Model (probabilistic) ───────────────────────────
    model_declaration: $ => seq(
      'model',
      field('name', $.identifier),
      '{',
      repeat($.model_item),
      '}',
    ),

    model_item: $ => choice(
      $.prior_statement,
      $.observe_statement,
      $.posterior_statement,
    ),

    prior_statement: $ => seq(
      'prior',
      field('name', $.identifier),
      ':',
      $._expression,
    ),

    observe_statement: $ => seq(
      'observe',
      $._expression,
    ),

    posterior_statement: $ => seq(
      'posterior',
      '=',
      $._expression,
    ),

    // ─── Meta (self-modification) ────────────────────────
    meta_block: $ => seq(
      'meta',
      field('action', $.identifier),
      '(',
      field('target', $._expression),
      ')',
      '{',
      repeat($.meta_item),
      '}',
    ),

    meta_item: $ => choice(
      $.property_pair,
      $.block_field,
    ),

    block_field: $ => prec(1, seq(
      $.identifier,
      ':',
      '{',
      repeat(choice($._expression, $.property_pair)),
      '}',
    )),

    // ─── Multi-resolution annotation ─────────────────────
    annotation: $ => prec.right(seq(
      '@',
      $.identifier,
      optional(seq('(', commaSep1($._expression), ')')),
    )),

    // ─── Let binding ─────────────────────────────────────
    let_binding: $ => seq(
      'let',
      field('name', $.identifier),
      optional(seq(':', field('type', $._type_expr))),
      '=',
      field('value', $._expression),
    ),

    // ─── Properties / Semantic / Proof blocks ────────────
    properties_block: $ => seq(
      'properties', ':', '{',
      repeat($.property_pair),
      '}',
    ),

    semantic_block: $ => seq(
      'semantic', ':', '{',
      repeat($.semantic_item),
      '}',
    ),

    semantic_item: $ => choice(
      seq('description', ':', $.string_literal),
      seq('formal', ':', $._expression),
    ),

    proof_obligations_block: $ => seq(
      'proof_obligations', ':', '{',
      repeat($.proof_obligation),
      '}',
    ),

    proof_obligation: $ => seq(
      field('name', $.identifier),
      ':',
      field('body', $._expression),
    ),

    property_pair: $ => seq(
      field('key', $.identifier),
      ':',
      field('value', $._expression),
    ),

    // ─── Expressions ─────────────────────────────────────
    _expression: $ => choice(
      $.binary_expression,
      $.unary_expression,
      $.pipeline_expression,
      $.call_expression,
      $.field_expression,
      $.index_expression,
      $.relational_expression,
      $.lambda_expression,
      $.if_expression,
      $.match_expression,
      $.branch_expression,
      $.do_block,
      $.quantifier_expression,
      $.number_literal,
      $.string_literal,
      $.boolean_literal,
      $.array_literal,
      $.set_literal,
      $.record_literal,
      $.unit_literal,
      $.identifier,
      $.annotation,
      $.parenthesized_expression,
    ),

    parenthesized_expression: $ => seq('(', $._expression, ')'),

    binary_expression: $ => choice(
      ...[
        ['+', PREC.ADD],
        ['-', PREC.ADD],
        ['*', PREC.MULT],
        ['/', PREC.MULT],
        ['%', PREC.MULT],
        ['=', PREC.COMPARE],
        ['<', PREC.COMPARE],
        ['>', PREC.COMPARE],
        ['<=', PREC.COMPARE],
        ['>=', PREC.COMPARE],
        ['≤', PREC.COMPARE],
        ['≥', PREC.COMPARE],
        ['≠', PREC.COMPARE],
        ['!=', PREC.COMPARE],
        ['∧', PREC.AND],
        ['and', PREC.AND],
        ['∨', PREC.OR],
        ['or', PREC.OR],
        ['⟹', PREC.IMPLIES],
        ['implies', PREC.IMPLIES],
        ['∈', PREC.MEMBERSHIP],
        ['in', PREC.MEMBERSHIP],
        ['∉', PREC.MEMBERSHIP],
        ['not_in', PREC.MEMBERSHIP],
        ['++', PREC.ADD],
        ['~', PREC.COMPARE],
      ].map(([op, prec_val]) =>
        prec.left(prec_val, seq(
          field('left', $._expression),
          field('operator', op),
          field('right', $._expression),
        ))
      ),
    ),

    unary_expression: $ => prec(PREC.UNARY, seq(
      field('operator', choice('-', '¬', 'not')),
      field('operand', $._expression),
    )),

    pipeline_expression: $ => prec.left(PREC.PIPELINE, seq(
      field('left', $._expression),
      '|>',
      field('right', $._expression),
    )),

    call_expression: $ => prec(PREC.CALL, seq(
      field('function', $._expression),
      '(',
      optional(commaSep1(choice($.property_pair, $._expression))),
      ')',
    )),

    field_expression: $ => prec.left(PREC.FIELD, seq(
      field('object', $._expression),
      '.',
      field('field', $.identifier),
    )),

    index_expression: $ => prec(PREC.INDEX, seq(
      field('object', $._expression),
      '[',
      field('index', $._expression),
      optional(seq(':', field('end', $._expression))),
      ']',
    )),

    // ─── Relational algebra ──────────────────────────────
    relational_expression: $ => choice(
      $.select_expression,
      $.project_expression,
      $.join_expression,
      $.group_by_expression,
    ),

    select_expression: $ => prec.right(PREC.CALL, seq(
      choice('σ', 'select'),
      '(',
      $._expression,
      ')',
      optional(seq('(', $._expression, ')')),
    )),

    project_expression: $ => prec.right(PREC.CALL, seq(
      choice('π', 'project'),
      '[',
      commaSep1($._expression),
      ']',
      optional(seq('(', $._expression, ')')),
    )),

    join_expression: $ => prec.left(PREC.MULT, seq(
      field('left', $._expression),
      choice('⋈', 'join'),
      optional(seq('(', $._expression, ')')),
      field('right', $._expression),
    )),

    group_by_expression: $ => prec.right(PREC.CALL, seq(
      choice('γ', 'group_by'),
      '[',
      commaSep1($._expression),
      optional(seq(';', commaSep1($._expression))),
      ']',
      optional(seq('(', $._expression, ')')),
    )),

    // ─── Lambda ──────────────────────────────────────────
    lambda_expression: $ => prec.right(seq(
      choice('λ', 'fn'),
      field('param', $.identifier),
      optional(seq(':', field('type', $._type_expr))),
      choice('→', '->'),
      field('body', $._expression),
    )),

    // ─── Control flow ────────────────────────────────────
    if_expression: $ => prec.right(seq(
      'if',
      field('condition', $._expression),
      'then',
      field('then', $._expression),
      optional(seq('else', field('else', $._expression))),
    )),

    match_expression: $ => seq(
      'match',
      field('scrutinee', $._expression),
      '{',
      repeat($.match_arm),
      '}',
    ),

    match_arm: $ => seq(
      field('pattern', $._pattern),
      choice('→', '->'),
      field('body', $._expression),
    ),

    _pattern: $ => choice(
      $.identifier,
      $.number_literal,
      $.string_literal,
      $.boolean_literal,
      '_',
      $.constructor_pattern,
    ),

    constructor_pattern: $ => seq(
      $.identifier,
      '(',
      commaSep1($._pattern),
      ')',
    ),

    branch_expression: $ => seq(
      'branch',
      field('distribution', $._expression),
      '{',
      repeat($.branch_arm),
      '}',
    ),

    branch_arm: $ => seq(
      field('pattern', $._expression),
      choice('→', '->'),
      field('body', $._expression),
    ),

    // ─── Do block (monadic) ──────────────────────────────
    do_block: $ => seq(
      'do',
      '{',
      repeat($.do_statement),
      '}',
    ),

    do_statement: $ => choice(
      $.bind_statement,
      $.yield_statement,
      $.let_binding,
      $._expression,
    ),

    bind_statement: $ => seq(
      field('name', $.identifier),
      choice('←', '<-'),
      field('value', $._expression),
      optional('?'),
    ),

    yield_statement: $ => seq('yield', $._expression),

    // ─── Quantifier expressions ──────────────────────────
    quantifier_expression: $ => prec.right(PREC.UNARY, seq(
      choice('∀', 'forall', '∃', 'exists'),
      field('variable', $.identifier),
      choice('∈', 'in'),
      field('domain', $._expression),
      optional(seq(choice('→', '->'), field('body', $._expression))),
    )),

    // ─── Literals ────────────────────────────────────────
    number_literal: $ => /\d+(\.\d+)?/,

    string_literal: $ => seq(
      '"',
      repeat(choice(
        /[^"\\]+/,
        /\\./,
      )),
      '"',
    ),

    boolean_literal: $ => choice('true', 'false'),

    array_literal: $ => seq('[', commaSep($._expression), ']'),

    set_literal: $ => seq('{', commaSep1($._expression), '}'),

    record_literal: $ => seq('{', commaSep1($.property_pair), '}'),

    // number.unit syntax: 200.ms, 4.GiB, 19.99.USD
    unit_literal: $ => prec(PREC.POSTFIX, seq(
      $.number_literal,
      '.',
      $.identifier,
    )),

    // ─── Identifier ──────────────────────────────────────
    identifier: $ => /[a-zA-Z_\u{00C0}-\u{024F}\u{0370}-\u{03FF}\u{2100}-\u{214F}\u{2200}-\u{22FF}][a-zA-Z0-9_\u{00C0}-\u{024F}\u{0370}-\u{03FF}\u{2100}-\u{214F}\u{2200}-\u{22FF}]*/u,

    // ─── Comments ────────────────────────────────────────
    line_comment: $ => seq('--', /[^\n]*/),
  },
});

function commaSep(rule) {
  return optional(commaSep1(rule));
}

function commaSep1(rule) {
  return seq(rule, repeat(seq(',', rule)), optional(','));
}
