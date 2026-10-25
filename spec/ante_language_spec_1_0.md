# The "Ante" Programming Language — Specification, Edition 1.0

**Status:** Final draft for implementation. Normative for the reference compiler `antec`.  
**Revision:** Edition 1.0, draft r1. Vocabulary may still change before `antec` 1.0.0, under the rules in `docs/VERSIONING.md` (§23.5).  
**Language:** "Ante" ("ante" = "means"). Source files use the extension `.ante`; the compiler is `antec`; the package manifest is `ante.toml`.  

---

## Contents

- Part I — Introduction: §1 Scope and conformance, §2 Notation, §3 Design principles
- Part II — Lexical structure: §4
- Part III — Syntax: §5 Grammar, §6 Disambiguation rules
- Part IV — Static semantics: §7 Programs and modules, §8 Names and scopes, §9 Types, §10 Type checking, §11 Calls, §12 Pipes, §13 Traits and generics, §14 Patterns, §15 Ownership and borrowing, §16 Constant evaluation
- Part V — Dynamic semantics: §17 Values and evaluation, §18 Arithmetic, §19 Control flow and drop, §20 Program execution and traps
- Part VI — Standard library: §21 Prelude and core modules
- Part VII — Interop: §22 C FFI and `naa_risk`
- Part VIII — Tooling: §23 `antec`, manifest, tests, diagnostics
- Annex T — PyTorch binding (optional conformance level)
- Annex P — Python bridge (optional conformance level)
- Appendix A — Vocabulary and romanization
- Appendix B — Diagnostic codes
- Appendix C — Conformance suite
- Appendix D — Example programs
- Appendix E — Implementation roadmap for `antec`
- Appendix F — Deferred features
- Appendix G — Change history
- Appendix H — References
- Appendix I — Self-hosting readiness

---

# Part I — Introduction

## 1. Scope and conformance

1.1 This document defines the syntax, static semantics and dynamic semantics of "Ante" Edition 1.0 and the minimum standard library that every conforming implementation provides.

1.2 **Conformance levels.**

| Level | Requires |
|---|---|
| **"Ante" Core 1.0** | Parts I–VIII and Appendix B. Every implementation must provide this level. |
| **"Ante" Torch 1.0** | Core plus Annex T |
| **"Ante" Python 1.0** | Core plus Annex P |

1.3 A **conforming implementation**:
- accepts every program that this spec defines as valid and gives it the behavior specified here;
- rejects every invalid program with at least one diagnostic carrying the error code and primary source span listed in Appendix B (the prose of the message is not normative);
- documents every **implementation-defined** behavior (listed in §20.6).

1.4 **Undefined behavior** exists only inside `naa_risk:` blocks (§22) when a programmer-maintained invariant is violated. A program that contains no `naa_risk:` block has no undefined behavior.

1.5 The words **must**, **must not** and **may** are normative. Notes and examples are informative.

## 2. Notation

2.1 Grammar uses EBNF: `::=` defines, `|` separates alternatives, `( )` groups, `?` means optional, `*` means zero or more, `+` means one or more. Quoted text `'aithe'` is a literal token. UPPERCASE names (`NAME`, `INT`, `NEWLINE`, `INDENT`, `DEDENT`) are token classes from §4.

2.2 Typing rules are written `Γ ⊢ e : T` ("in environment Γ, expression `e` has type `T`"). Γ maps names to types and records mutability.

2.3 Code examples use the `ante` fence tag. An example marked `# invalid: E0xxx` must be rejected with that code.

## 3. Design principles

1. **Head-final, case-marked syntax.** Arguments come before the action, each tagged with a Telugu case marker (`ni`, `tho`, `ki`, …). The condition comes before `aithe`, the value before `icchey`, and the destination before `pettu`. The one deliberate exception is the definitional copula `X ante Y` ("X means Y"), as in spoken Telugu.
2. **The grammar is formal, not natural language.** Case markers are tokens. Binding never depends on guessing.
3. **Statically typed and natively compiled** via LLVM. Programs that don't import `python` need no Python runtime.
4. **Memory-safe by default.** Ownership, borrowing, bounds checks and checked arithmetic. Unsafety is confined to `naa_risk:`.
5. **Predictable.** Left-to-right evaluation, no implicit conversions, the same arithmetic in debug and release. The optimizer may change speed but not behavior, except inside an explicit `@vegam` opt-in.
6. **Spoken Tinglish vocabulary** (Appendix A): the words people use at home, English technical nouns with Telugu grammar, and puns only when they teach the meaning.

---

# Part II — Lexical structure

## 4. Lexical structure

### 4.1 Source text

4.1.1 Source is UTF-8. A leading byte-order mark is ignored. CRLF and CR are normalized to LF before tokenizing. Invalid UTF-8 is error E0101. Implementations preserve original byte offsets for diagnostics.

4.1.2 Outside string, character and f-string literals and comments, only ASCII characters may appear (E0101). Unicode identifiers are deferred (Appendix F).

### 4.2 Line structure

The tokenizer turns physical lines into **logical lines** ending in `NEWLINE`, and emits `INDENT`/`DEDENT` tokens.

4.2.1 **Blank lines** (only whitespace and/or a comment) produce no tokens.

4.2.2 **Indentation** is the count of leading spaces. A tab in leading whitespace is E0102. Canonical style is 4 spaces per level, but any consistent increase is accepted.

4.2.3 **Implicit joining.** Physical lines are joined into one logical line when any of these holds:
- (a) the line break is inside an open `(`, `[` or `{`;
- (b) the previous physical line ends with `,` or `|>`;
- (c) the next physical line starts with `|>`;
- (d) the next physical line is indented **more** than the first physical line of the current logical line, **and** the current logical line does not end with `:`.

Joined lines do not affect the indentation stack. A backslash at the end of a line is not a continuation (E0201).

4.2.4 **Suites.** When a logical line ends with `:`, the next logical line must be indented more (E0201 otherwise). The tokenizer pushes that indentation and emits `INDENT`. When a logical line is indented less than the top of the stack, the tokenizer pops and emits one `DEDENT` per popped level. The new indentation must equal a level on the stack (E0103). At end of file it emits `NEWLINE` if needed, then one `DEDENT` for each open level, then `EOF`.

### 4.3 Comments

`#` starts a comment that runs to the end of the line (outside literals). `##` immediately before a declaration is a documentation comment. Doc comments are tooling metadata with no semantics. There are no block comments.

### 4.4 Identifiers and keywords

4.4.1 `NAME ::= [A-Za-z_][A-Za-z0-9_]*`, excluding the keywords below. A lone `_` is the token `_`. Keywords are matched as whole tokens, so `naa_risk` is one keyword, while `ledhu_ayithe` and `sarainavi_theesuko` are ordinary identifiers.

4.4.2 **Keywords** (lowercase, case-sensitive; using one as an identifier is E0307):

| Group | Keywords |
|---|---|
| Declarations | `ante` `rakam` `edho_okati` `gunam` `panulu` `undhi` `andhariki` `bayata` `thechuko` |
| Bindings and statements | `maare` `naa` `pettu` `icchey` `aapey` `vadhiley` `vellagane` `naa_risk` `theesuko` |
| Control | `aithe` `ledhante` `unnanthasepu` `okko` `batti` |
| Case markers | `ni` `ki` `tho` `lo` `nunchi` `kosam` `ga` |
| Ranges | `lopu` `varaku` |
| Parameters | `migathavi` `migatha_perlu` `peru_thone` |
| Literals and constructors | `avunu` `kaadhu` `ledhu` `sare` `thappu` |
| Types | `ankam` `ankam8` `ankam16` `ankam32` `ankam64` `aasajeevi` `amsam` `amsam32` `amsam64` `ato_ito` `padham` `aksharam` `jatha` `jodi` `undochu` `varusa` `mukka` `sare_thappu` `pani` `pette` `khali` `raadhu` `idhi` |
| Reserved for later editions | `edhaina` `kitiki` `panchukunna` `gurinchi` `async` `await` `yield` |

`undhi` is both the optional constructor (`undhi(v)`) and the implementation keyword (`… gunam undhi:`). The two positions never overlap.

### 4.5 Literals

```ebnf
INT        ::= DEC | '0b' [01_]+ | '0o' [0-7_]+ | '0x' [0-9A-Fa-f_]+
DEC        ::= [0-9] [0-9_]*
FLOAT      ::= DEC '.' DEC EXP? | DEC EXP
EXP        ::= [eE] [+-]? DEC
STRING     ::= '"' (CHARSTR | ESCAPE)* '"'
CHAR       ::= "'" (CHARCHR | ESCAPE) "'"
FSTRING    ::= 'f"' (CHARF | ESCAPE | '{{' | '}}' | '{' clause '}')* '"'
ESCAPE     ::= '\\' | '\"' | "\'" | '\n' | '\t' | '\r' | '\0' | '\u{' HEX{1,6} '}'
```

- Digit separators `_` may not start or end a digit sequence, may not appear twice in a row, and may not follow a base prefix directly (E0106). A number immediately followed by a letter or `_` is E0106. After a `.` token, digits lex as an integer only, so `t.0.1` is two tuple indices, not a float.
- A literal has no sign. `-5` is unary minus applied to `5`, folded per §10.4.
- `\u{…}` must be a Unicode scalar value, so surrogates and values above U+10FFFF are E0108.
- Strings may contain any Unicode except unescaped `"`, `\` or a newline (E0104 for an unterminated string). Adjacent strings are not concatenated.
- In an `f"…"` string, `{ clause }` interpolates (§10.6) and `{{` / `}}` produce literal braces. A `{ … }` may not contain a string literal.
- `CHAR` holds exactly one Unicode scalar value and has type `aksharam` (E0109 otherwise).
- In an f-string, a lone `}`, an unclosed `{` or an empty `{}` is E0110.
- `avunu`/`kaadhu` are the `ato_ito` literals.

### 4.6 Operators and punctuation

Tokens are formed by maximal munch:

```text
+  -  *  /  %  @  &  |  ^  ~  !  <<  >>
== != <  <= >  >=  &&  ||
+= -= *= /=
=  -> => |> ?  ... .  ,  :  ;
(  )  [  ]  {  }
```

`;` is valid only inside an array type `[T; N]` (E0206 elsewhere). `@` is the matmul operator in an expression and the annotation sigil when it is the first token of a logical line (§5.2).

---

# Part III — Syntax

## 5. Grammar

### 5.1 Modules and items

```ebnf
module        ::= item* EOF
item          ::= import
                | annotation* 'andhariki'? declaration
annotation    ::= '@' NAME annotation_arg? NEWLINE
annotation_arg ::= STRING | NAME (',' NAME)*

import        ::= module_path 'thechuko' NEWLINE
                | module_path 'nunchi' NAME (',' NAME)* 'thechuko' NEWLINE
                | module_path 'ni' NAME 'ga' 'thechuko' NEWLINE
module_path   ::= NAME ('.' NAME)*

declaration   ::= fn_decl | const_decl | record_decl | enum_decl
                | methods_decl | trait_decl | impl_decl | extern_block

fn_decl       ::= fn_sig 'ante' ':' block
fn_sig        ::= NAME generic_params? '(' params? ')' '->' type
generic_params ::= '[' generic_param (',' generic_param)* ','? ']'
generic_param ::= NAME (':' bounds)?
                | NAME ':' 'ankam'                         /* const generic */
bounds        ::= type_path ('+' type_path)*
params        ::= param (',' param)* ','?
param         ::= 'naa' ':' type
                | ('migathavi' | 'migatha_perlu')? NAME ':' type role? 'peru_thone'? ('=' expr)?
role          ::= 'ni' | 'ki' | 'tho' | 'lo' | 'nunchi' | 'kosam' | 'batti'

const_decl    ::= NAME ':' type 'ante' expr NEWLINE

record_decl   ::= 'rakam' NAME generic_params? 'ante' ':' NEWLINE INDENT field+ DEDENT
field         ::= 'andhariki'? NAME ':' type NEWLINE

enum_decl     ::= 'edho_okati' NAME generic_params? 'ante' ':' NEWLINE INDENT variant+ DEDENT
variant       ::= NAME ('(' type (',' type)* ','? ')')? NEWLINE

methods_decl  ::= generic_params? type_path 'panulu' 'ante' ':' NEWLINE
                  INDENT (annotation* 'andhariki'? fn_decl)+ DEDENT
trait_decl    ::= 'gunam' NAME generic_params? 'ante' ':' NEWLINE
                  INDENT (fn_sig NEWLINE | fn_decl)+ DEDENT
impl_decl     ::= generic_params? type 'ki' type_path 'gunam' 'undhi'
                  ( ':' NEWLINE INDENT fn_decl+ DEDENT | NEWLINE )
extern_block  ::= 'bayata' STRING 'ante' ':' NEWLINE INDENT (fn_sig NEWLINE)+ DEDENT
```

### 5.2 Statements

```ebnf
block         ::= NEWLINE INDENT stmt+ DEDENT
stmt          ::= binding | assign | compound | return | if_stmt | while_stmt
                | for_stmt | match_stmt | break | continue | defer | unsafe_block
                | expr_stmt

binding       ::= 'maare'? bind_target (':' type)? 'ante' pipe_expr NEWLINE
bind_target   ::= NAME | '(' NAME (',' NAME)+ ','? ')'
assign        ::= place 'lo' pipe_expr 'pettu' NEWLINE
compound      ::= place ('+=' | '-=' | '*=' | '/=') pipe_expr NEWLINE
place         ::= '*'* postfix
return        ::= pipe_expr? 'icchey' NEWLINE
if_stmt       ::= pipe_expr 'aithe' ':' block
                  ('ledhante' pipe_expr 'aithe' ':' block)*
                  ('ledhante' ':' block)?
while_stmt    ::= pipe_expr 'unnanthasepu' ':' block
for_stmt      ::= pipe_expr 'lo' 'okko' pattern 'ki' ':' block
                | range 'okko' pattern 'ki' ':' block
match_stmt    ::= pipe_expr 'batti' ':' NEWLINE INDENT arm+ DEDENT
arm           ::= pattern 'aithe' ':' block
break         ::= 'aapey' NEWLINE
continue      ::= 'vadhiley' NEWLINE
defer         ::= 'vellagane' ':' block
unsafe_block  ::= 'naa_risk' ':' block
expr_stmt     ::= pipe_expr NEWLINE
```

### 5.3 Expressions

```ebnf
pipe_expr     ::= clause ('|>' clause)*
clause        ::= phrase (',' phrase)* callee          /* a call */
                | expr                                  /* a plain expression */
phrase        ::= '...' expr role? label?
                | '_' role? label?
                | expr role? label?
label         ::= NAME 'ga'
callee        ::= path type_args? '(' ')' '?'?
path          ::= (NAME | 'naa' | type_kw) ('.' NAME)*  /* type_kw: varusa.kotthadhi() */
type_kw       ::= 'varusa' | 'jodi' | 'pette' | 'padham' | 'mukka' | 'undochu' | 'sare_thappu'

expr          ::= range | closure | binexpr
range         ::= binexpr 'nunchi' binexpr ('lopu' | 'varaku')
closure       ::= 'theesuko'? '(' (cparam (',' cparam)*)? ')' '=>' pipe_expr
cparam        ::= NAME (':' type)? role?

binexpr       ::= unary (BINOP unary)*                   /* precedence: §5.4 */
unary         ::= ('-' | '!' | '~' | '*' | '&' 'maare'?) unary | postfix
postfix       ::= primary postfix_op*
postfix_op    ::= '.' NAME | '.' INT | '[' index ']' | type_args? '(' ')' | '?'
index         ::= pipe_expr | range | pipe_expr (',' pipe_expr)+   /* multi-index: tensors only */
primary       ::= INT | FLOAT | STRING | CHAR | FSTRING
                | 'avunu' | 'kaadhu' | 'ledhu' | 'sare' | '_'
                | path
                | ctor
                | record_lit
                | '(' ')' | '(' pipe_expr ')' | '(' pipe_expr ',' ')'
                | '(' pipe_expr (',' pipe_expr)+ ','? ')'
                | '[' (pipe_expr (',' pipe_expr)* ','?)? ']'
                | '{' (pipe_expr ':' pipe_expr (',' pipe_expr ':' pipe_expr)* ','?)? '}'
ctor          ::= ('undhi' | 'sare' | 'thappu' | path) '(' pipe_expr (',' pipe_expr)* ','? ')'
record_lit    ::= path '{' (NAME ':' pipe_expr (',' NAME ':' pipe_expr)* ','?)? '}'
type_args     ::= '[' type (',' type)* ','? ']'
```

### 5.4 Operator precedence

From tightest to loosest. All binary levels associate left except comparison, which does not chain.

| Level | Operators | Notes |
|---|---|---|
| 1 | postfix `.f` `[i]` `()` `?` | |
| 2 | prefix `-` `!` `~` `*` `&` `&maare` | |
| 3 | `*` `/` `%` `@` | |
| 4 | `+` `-` | |
| 5 | `<<` `>>` | |
| 6 | `&` | bitwise |
| 7 | `^` | |
| 8 | `\|` | |
| 9 | `<` `<=` `>` `>=` | non-associative; `a < b < c` is E0417 |
| 10 | `==` `!=` | non-associative |
| 11 | `&&` | short-circuit |
| 12 | `\|\|` | short-circuit |
| 13 | range `nunchi … lopu/varaku` | |
| 14 | call clause (phrases + callee) | |
| 15 | `\|>` | |

A call clause can't be an operand of a binary operator without parentheses: write `(a ni f()) + b`. Phrase operands are `expr`, so `a + b ni f()` means `(a + b) ni f()`.

### 5.5 Patterns

```ebnf
pattern       ::= alt_pattern ('|' alt_pattern)*            /* OR-pattern */
alt_pattern   ::= '_'
                | literal_pat
                | range_pat
                | 'maare'? NAME                            /* binding */
                | 'undhi' '(' pattern ')' | 'ledhu'
                | 'sare' '(' pattern ')' | 'thappu' '(' pattern ')'
                | path ('(' pattern (',' pattern)* ','? ')')?   /* qualified variant */
                | path '{' field_pat (',' field_pat)* (',' '..')? ','? '}'
                | '(' ')' | '(' pattern ',' ')' | '(' pattern (',' pattern)+ ','? ')'
literal_pat   ::= '-'? INT | '-'? FLOAT | STRING | CHAR | 'avunu' | 'kaadhu'
range_pat     ::= ('-'? INT | CHAR) 'nunchi' ('-'? INT | CHAR) ('lopu' | 'varaku')
field_pat     ::= NAME (':' pattern)?
```

**OR-patterns.** `Token.Plus | Token.Minus aithe:` matches either alternative. Every alternative must bind the same names with the same types (E0706). **Range patterns.** `'a' nunchi 'z' varaku aithe:` matches an integer or `aksharam` in the range. The bounds must be literals of the scrutinee's type, with the lower bound ≤ the upper bound (E0707).

A bare `NAME` pattern is always a new binding. A variant is always written as a qualified path with at least one `.` (`Rangu.Erupu`), except the prelude constructors `undhi`, `ledhu`, `sare` and `thappu`. Float literal patterns are E0705.

### 5.6 Types

```ebnf
type          ::= '&' 'maare'? type
                | '*' 'maare'? type                        /* raw pointer, §22 */
                | 'aasajeevi' int_kw
                | int_kw | 'amsam' | 'amsam32' | 'amsam64'
                | 'ato_ito' | 'padham' | 'aksharam' | 'khali' | 'raadhu' | 'idhi'
                | '[' type ';' const_expr ']'
                | 'jatha' '[' (type (',' type)*)? ']'
                | ('varusa' | 'mukka' | 'undochu' | 'pette') '[' type ']'
                | ('jodi' | 'sare_thappu') '[' type ',' type ']'
                | 'pani' '[' '(' (type role? (',' type role?)*)? ')' '->' type ']'
                | type_path
type_path     ::= path type_args?
int_kw        ::= 'ankam' | 'ankam8' | 'ankam16' | 'ankam32' | 'ankam64'
```

## 6. Disambiguation rules

These rules are normative. A parser may implement them however it likes (recursive descent with bounded lookahead and local backtracking is sufficient), but it must produce the parse they describe.

**D1. Item kind** (module scope and inside `panulu`/`gunam` bodies). Look at the tokens of the logical line outside brackets:
1. last token is `thechuko` → import;
2. first token is `rakam`, `edho_okati`, `gunam` or `bayata` → that declaration;
3. contains `panulu` immediately followed by `ante :` → methods block;
4. contains `gunam undhi` → implementation;
5. `NAME :` at start → constant;
6. `NAME (` or `NAME [ … ] (` at start → function declaration.

Anything else at module scope is E0209 (executable statement at module level). A function declaration inside a function body is E0211.

**D2. Statement kind** (inside a block). Look at the tokens of the logical line outside brackets:
1. first token `aapey` / `vadhiley` → break / continue;
2. last token `:` → a header, classified by the token before `:`: `aithe` → if (or an arm inside a `batti` block; a leading `ledhante` makes it an else-if), `ledhante` → else, `unnanthasepu` → while, `ki` → for, `batti` → match, `naa_risk` → unsafe, `vellagane` → defer;
3. last token `pettu` → assign;
4. last token `icchey` → return;
5. first token `maare`, or a line starting `NAME ante`, `NAME :` or `( NAME , … ) ante` → binding;
6. contains a top-level `+=`, `-=`, `*=` or `/=` → compound assignment;
7. otherwise → expression statement.

**D3. Phrase lists.** When parsing a `clause`, parse a phrase operand as `expr`. Then:
- If the next token is a role, it is that phrase's role, **except** `lo` followed by `okko` (a for-loop, D2) and `nunchi` when D5 forms a range.
- `NAME ga` after the operand (and role) is a label.
- `,` continues the phrase list.
- Otherwise the phrase list ends. If it has exactly one phrase with no role, label, hole or spread, and the next token can't begin a `callee`, the clause is a plain `expr`. Otherwise the next tokens must form a `callee` (E0202 "verb ekkada?"; E0203 if a comma is missing).

**D4. Commas in bracketed lists.** Inside `( … )`, `[ … ]`, `{ … }` and a list of indices, a comma belongs to a phrase list only if the phrase list ends with a callee. Otherwise the comma separates list elements. So `(a, b f())` is the call `f(a, b)`, and `(a, b)` is a tuple. The rule is greedy from the first element: in `Komma(x, 5, y ni f())` the whole argument list becomes one call. To make a call clause a single element of a multi-element list, parenthesize it: `Komma(x, 5, (y ni f()))`. `antec fmt` inserts these parentheses.

**D5. Ranges.** After `binexpr nunchi`, if a `binexpr` follows and is then followed by `lopu` or `varaku`, the whole is a `range`. Otherwise `nunchi` is a role.

**D6. Callee vs operand.** In `x f()`, `f()` is the callee: postfix parsing of `x` stops at `NAME`. A zero-argument call `f()` alone is a plain expression. `x f() g()` is E0201, because nested calls need parentheses or a pipe.

**D7. Generic arguments vs indexing.** `postfix '[' … ']'` directly followed by `( )` is a generic call, and the bracket contents are parsed as types. Otherwise it's indexing. To index and then call, write `(handlers[0])()`.

**D8. Closures vs grouping.** `(` … `)` followed by `=>` is a closure parameter list. Otherwise it's grouping or a tuple. A closure body extends as far as a `pipe_expr` can, so a closure used as an argument phrase must be parenthesized: `xs, ((x) => x * 2) ni prathi_dhaaniki()`.

**D9. Record literals.** `path {` with no line break between them is a record literal. A `{` that starts an operand on its own is a map literal.

**D10. Constructor form.** `name(args)` with a non-empty argument list is a constructor (`ctor`). Semantic analysis rejects it with E0204 unless `name` resolves to a variant or is a prelude constructor. **No function is ever called as `f(x)`.**

**D11. `?` placement.** In a call clause, `?` after the callee's `()` applies to the result of the whole call: `x ni f()?` means `(x ni f())?`.

**D12. Annotations.** `@` as the first token of a logical line starts an annotation, which must be followed by a declaration (E0208). Anywhere else, `@` is matmul.

---

# Part IV — Static semantics

## 7. Programs and modules

7.1 **Packages.** A package is a directory containing `ante.toml` and `src/`. Each file `src/a/b.ante` is the module `a.b`. Module names must be valid `NAME`s. The entry module is `src/modhalu.ante` for executables and `src/lib.ante` for libraries, unless `ante.toml` says otherwise.

7.2 **Contents.** A module contains only imports and declarations (D1). Module-level constants must be constant-evaluable (§16) and carry a type annotation. There is no module-level mutable state and no module initialization code.

7.3 **Imports.** `m thechuko` binds module `m` as a namespace. `m nunchi a, b thechuko` binds the public items `a` and `b`. `m ni k ga thechuko` binds `m` under the alias `k`. Imports are module-scoped and order-independent. Importing a private item is E0303. An unknown module is E0901. The names `python` and `torch` are reserved module names (Annexes P and T).

7.4 **Visibility.** Items and record fields are module-private unless marked `andhariki` ("for everyone"). A public signature mentioning a private type is E0303. Methods in a `panulu` block are private unless marked `andhariki`. Trait methods are as visible as the trait.

7.5 **Entry point.** An executable has exactly one `modhalu() -> khali` or `modhalu() -> sare_thappu[khali, E]` in its entry module (E0903 missing, E0904 multiple). For the second form, `E` must implement `Choopinchochu`.

7.6 **Order independence.** Name resolution at module scope doesn't depend on declaration order. Modules may import each other in cycles: a module's types and functions may refer to items of a module that imports it. Only constants must form an acyclic dependency graph (E0902).

## 8. Names and scopes

8.1 Scopes are nested: prelude ⊃ module ⊃ function ⊃ block ⊃ arm/loop body/closure. Each block statement opens a new scope for its suite.

8.2 A binding is visible from the end of its own statement to the end of its scope. Using a name before its binding is E0306.

8.3 **Shadowing.** A binding may shadow a name from an *enclosing* scope. Redefining a name in the same scope is E0308. Shadowing never ends a borrow early.

8.4 **Namespaces.** Each module has one value namespace (functions, constants, local bindings) and one type namespace (records, enums, traits, imported modules). Defining the same name twice in one namespace is E0302. Variants live inside their enum (`Rangu.Erupu`). Methods live inside their type.

8.5 **Prelude.** These are in scope in every module without importing:
- all built-in types (§9.1), plus `Hasher`, `Raatha`, `gumpu`, `gajibiji_jodi`, `Paridhi`, `ParseThappu` and `Utf8Thappu`;
- the traits of §13.5;
- `Kramam` and its variants `Thakkuva`, `Samaanam`, `Ekkuva`;
- the constructors `undhi`, `ledhu`, `sare`, `thappu`;
- the functions of §21.1 and the iteration adapters of §21.4.2.

Module items shadow prelude names. Standard library modules (`lekkalu`, `file`, `system`, `samayam`, `memory`) and the generated bindings (`torch`, `python`) must be imported before use.

8.6 **Lookup order.** An unqualified name in value position resolves to, in order: a local (binding, parameter or pattern variable, innermost scope first); an item or import of the current module; inside a method with a `naa` receiver, a field or method of `idhi` (§13.4.4); a prelude value; then a type, module or generic parameter, whose remaining path segments name a variant, a module member, or an associated function. A name that's bound later in an enclosing block is E0306; any other unknown name is E0301.

## 9. Types

### 9.1 Built-in types

| Type | Meaning | CopyKottochu |
|---|---|---|
| `ankam8/16/32/64` | Two's-complement signed integers. `ankam` = `ankam64` | yes |
| `aasajeevi ankam8/16/32/64` | Unsigned integers ("optimist": never negative). `aasajeevi ankam` = `aasajeevi ankam64` | yes |
| `amsam32`, `amsam64` | IEEE 754 binary32/binary64. `amsam` = `amsam64` | yes |
| `ato_ito` | Boolean: `avunu` / `kaadhu` | yes |
| `aksharam` | Unicode scalar value | yes |
| `khali` | Unit type; single value `()` | yes |
| `raadhu` | Never type ("won't come"); has no values | — |
| `padham` | Immutable, owned, valid UTF-8 string | no (moves) |
| `jatha[T1, …, Tn]` | Tuple, n ≥ 0 | if all Ti are |
| `[T; N]` | Fixed array of N elements | if T is |
| `varusa[T]` | Owned growable vector ("a row") | no |
| `mukka[T]` | Contiguous slice ("a piece"). Unsized, so it appears only as `&mukka[T]` / `&maare mukka[T]` | — |
| `jodi[K, V]` | Owned insertion-ordered hash map | no |
| `undochu[T]` | Optional: `undhi(T)` or `ledhu` | if T is |
| `sare_thappu[T, E]` | Result: `sare(T)` or `thappu(E)` | if T and E are |
| `pette[T]` | Owned heap box ("a box"); used for recursive types | no |
| `pani[(P…) -> R]` | Callable value (§10.7) | if it has no captures |
| `&T` | Shared reference | yes |
| `&maare T` | Exclusive mutable reference | no |
| `*T`, `*maare T` | Raw pointers (§22) | yes |

`idhi` ("this") stands for the implementing type inside `panulu`, `gunam` and implementation blocks.

### 9.2 User-defined types

- `rakam` declares a record ("kind") with named fields. Records are nominal: two records with identical fields are different types.
- `edho_okati` declares a sum type ("one of these") with variants that may carry fields.
- A record or enum may contain itself only through `pette`, `varusa` or another indirection (E0418 for infinite size).

### 9.3 Reference-carrying types

9.3.1 A type is **reference-carrying (RC)** if it is `&T`, `&maare T`, or `mukka`-based, or if it is a closure that captures by borrow, or if it is a `jatha`/`undochu`/`sare_thappu` whose argument is RC, or if it is an intrinsic iterator over borrowed data (§21.4).

9.3.2 "Ante" 1.0 has no lifetime annotations. Instead, RC types are restricted:

- (a) RC types may appear in local bindings, temporaries, parameters and pattern bindings.
- (b) An RC type may **not** be a field of a `rakam` or `edho_okati`, an element of `varusa`/`[T;N]`/`jodi`, or the contents of a `pette` (E0415).
- (c) A function may return an RC type only if exactly one of its parameters (counting `naa`) has an RC type. The result then borrows from that parameter (E0608 otherwise).

### 9.4 Type equivalence and coercions

9.4.1 Types are equal when they are structurally equal after expanding the aliases `ankam` = `ankam64`, `aasajeevi ankam` = `aasajeevi ankam64` and `amsam` = `amsam64`.

9.4.2 The **only** implicit coercions are these, applied where an expected type is known:
- (a) `&maare T` → `&T`;
- (b) `&varusa[T]`, `&[T; N]` → `&mukka[T]`, and the same for `&maare` versions;
- (c) `&pette[T]` → `&T`, and the same for `&maare`;
- (d) `raadhu` → any type;
- (e) the automatic borrow at call sites (§11.3).

There are no numeric coercions.

## 10. Type checking

### 10.1 Approach

Each function body is checked independently against its fully annotated signature. Within a body, types of unannotated bindings and literals are found by unification. There is no let-polymorphism. When the body has been checked, any unresolved integer-literal variable defaults to `ankam64` and any float-literal variable to `amsam64`. Any other unresolved variable is E0404. Field access or method calls on a type that isn't yet known are E0404 ("type cheppu").

### 10.2 Bindings

```text
Γ ⊢ e : T        (with the annotation T, if given)
──────────────────────────────────────────────────
Γ ⊢ maare? x (: T)? ante e   ⟹   Γ, x : T [mutable if maare]
```

A tuple binding `(a, b) ante e` requires `e : jatha[A, B]`. Bindings are never uninitialized.

### 10.3 Statements

| Statement | Rule |
|---|---|
| `p lo e pettu` | `p` is a mutable place (§15.3) of type `T`; `e : T` |
| `p += e` (and `-=`, `*=`, `/=`) | same as `p lo (p + e) pettu` with `p` evaluated once; statement only |
| `e icchey` | `e : R` where `R` is the function's return type. A bare `icchey` requires `R = khali`. `sare icchey` requires `R = sare_thappu[khali, E]` |
| `c aithe: …` | `c : ato_ito` (E0403) |
| `c unnanthasepu: …` | `c : ato_ito` |
| `e lo okko p ki: …` / `r okko p ki: …` | `e` is iterable (§21.4) with item type `I`; pattern `p` is irrefutable against `I` |
| `e batti: arms` | each arm pattern checks against the type of `e`; arms must be exhaustive (§14) |
| `aapey` / `vadhiley` | only inside a loop body, not inside a closure (E0213) |

**Return check.** If `R` is neither `khali` nor `sare_thappu[khali, E]`, every control path must end in `icchey`, in a call returning `raadhu`, or in an infinite loop without `aapey` (E0410). A function whose return type is `khali` or `sare_thappu[khali, E]` and which reaches the end of its body returns `()` or `sare(())`.

### 10.4 Literals

| Literal | Type |
|---|---|
| `INT` | the expected integer type if there is one, else a fresh integer variable. The value must fit (E0402). Unary `-` on a literal checks the negated value, so `-128 : ankam8` is valid |
| `FLOAT` | the expected float type, else a float variable |
| `STRING`, `FSTRING` | `padham` |
| `CHAR` | `aksharam` |
| `avunu`, `kaadhu` | `ato_ito` |
| `()` | `khali` |
| `ledhu` | `undochu[T]`, `T` from context |
| `[e1, …]` | `varusa[T]`, or `[T; N]` when that's the expected type. An empty `[]` needs an expected type |
| `{k: v, …}` | `jodi[K, V]`. An empty `{}` needs an expected type. Duplicate constant keys are E0419 |
| `(e,)`, `(e1, e2, …)` | `jatha[…]` |

### 10.5 Operators

10.5.1 For built-in numeric types:
- `+ - * / %` require both operands to have the same numeric type and produce that type.
- `<< >>` take any integer left operand and any integer count, and produce the left operand's type.
- `& | ^ ~` require the same integer type.
- Comparisons require the same numeric type (or `ato_ito`/`aksharam` for `==`/`!=`) and produce `ato_ito`.
- `&& || !` require `ato_ito`.
- Mixing signed and unsigned is E0408. Any other mismatch is E0401.
- An operand of type `&T` or `&maare T`, where `T` is a built-in numeric type, `ato_ito` or `aksharam`, is dereferenced automatically. For `x: &ankam`, `x > 2` means `*x > 2`.

10.5.2 For other types, operators resolve to trait methods (§13.5): `+` → `Kalupochu.kalupu`, `-` → `Theeseyochu.theesey`, `*` → `Guninchochu.guninchu`, `/` → `Panchochu.panchu`, `%` → `Sheshamochu.shesham`, unary `-` → `Thippochu.thippu`, `@` → `MatmulCheyochu.matmul`, `==`/`!=` → `Polchochu.okateyna`, `< <= > >=` → `VarusaPettochu.polchu`. Operands are passed by automatic shared borrow. An operator with no implementation is E0407. `padham + padham` is provided and allocates.

10.5.3 Comparisons don't chain (E0417).

### 10.6 Other expressions

| Form | Typing |
|---|---|
| `x` | `Γ(x)`; a variant path `E.V` without fields has type `E` |
| `e.f` | auto-dereferences `e` through any number of `&`/`&maare`/`pette` until it reaches a record with field `f` (E0405). The field must be visible |
| `e.0`, `e.1` | `jatha` element |
| `e[i]` | `e` is (after auto-deref) `[T;N]`, `varusa[T]` or `mukka[T]`, and `i` is any integer type. Result is a place of type `T`. Indexing `padham` is E0413 |
| `e[a nunchi b lopu]` | same `e`; result is a place of type `mukka[T]` (unsized, so it must be borrowed) |
| `&e`, `&maare e` | `&T`, `&maare T`; `&maare` requires a mutable place (E0607) |
| `*e` | `e : &T`, `&maare T` or `pette[T]` gives `T`; raw pointers need `naa_risk` |
| `e?` | `e : sare_thappu[T, E]` gives `T`. The enclosing function must return `sare_thappu[_, F]` where `F = E` or `F` implements `NunchiRavochu[E]`, in which case the error is converted with `F.nunchi` (E0411, E0412) |
| `f"…{c}…"` | each `c : C` with `C: Choopinchochu`; result `padham` |
| `undhi(e)`, `sare(e)`, `thappu(e)`, `E.V(e…)` | the constructor's type, with arity checked (E0704) |
| `T{f: e, …}` | every field exactly once (E0416); fields must be visible |
| `a nunchi b lopu/varaku` | `a, b : I` for an integer type `I`; result `Paridhi[I]` (§21.4) |

### 10.7 Closures and `pani`

10.7.1 A closure `(x: T ni, …) => body` has type `pani[(T ni, …) -> R]`, where `R` is the type of `body`. Parameter types and roles may be left out when an expected `pani` type supplies them.

10.7.2 **Captures.**
- A closure captures by shared borrow any outer local it reads.
- It captures by mutable borrow any outer local it assigns or borrows with `&maare` (the local must be `maare`).
- With the `theesuko` ("take it") prefix, it captures every outer local it mentions by **move** (a copy for `CopyKottochu` types).

A closure with any borrow captures is RC (§9.3).

10.7.3 A named function used as a value has a `pani` type with its declared parameter roles. Calling a local of `pani` type uses the call syntax of §11 with the roles from its type.

## 11. Calls

### 11.1 Callee resolution

The callee `path` resolves to one of the following, in this order:
1. a local binding of `pani` type;
2. a method `recv.m`, where `recv` is a path resolving to a value (§13.4);
3. an associated function `Type.f`;
4. a function in scope or through a module path.

There is no overloading: each name resolves to exactly one declaration (E0302 for duplicates, E0305 for ambiguous trait methods).

### 11.2 Binding algorithm

Given a callee with formals F₁…Fₙ (excluding the `naa` receiver) and source phrases P₁…Pₘ:

1. **Classify phrases.** Each phrase is a tuple (operand, role-or-none, label-or-none, spread?, hole?, span). Unknown role words are already syntax errors.
2. **Named phrases first.** For each phrase with label `x`: there must be a formal named `x` (otherwise it goes to the `migatha_perlu` collector if there is one, else E0504). The phrase's role must equal the formal's role (E0502); an unmarked formal requires an unmarked phrase. Binding a formal twice is E0503.
3. **Unnamed phrases in source order.** Bind each to the **first** unbound fixed formal with the **same role** (unmarked matches unmarked only), in declaration order. Never choose by type. A formal marked `peru_thone` ("only by name") is skipped in this step (E0505 if it can only be filled this way).
4. **Collector.** Any unnamed phrases left over must have the role of the `migathavi` collector (E0502). They form a `&mukka[T]` in source order.
5. **Spreads.**
   - `...e` with a fixed-length type (`jatha`, `[T;N]`) expands into consecutive unnamed phrases with the spread's role.
   - `...e` with a runtime length (`varusa[T]`, `&mukka[T]`) may only feed the `migathavi` collector (E0506).
   - `...m ga` with `m : jodi[padham, V]` may only feed `migatha_perlu`, at most once per call.
6. **Defaults.** Fill unbound formals that have `= const` defaults. Any required formal left unbound is E0501 (message: "`f` ki `role` evaru ivvaledhu").
7. **Check.** Apply §11.3, then type-check each binding against its formal type.
8. **Lower.** The result is `Call(fn, [formal₁ := v, …])`. There is no runtime grammar processing.

### 11.3 Automatic borrows

When a formal has type `&T` and the bound phrase's operand has type `T` (or coerces to `T` per §9.4.2), the compiler inserts a shared borrow `&operand`. A temporary operand lives until the end of the enclosing statement. Formals of type `&maare T` are never borrowed automatically: an owned operand is E0610 ("`&maare x` ani raayi").

Method receivers (§13.4) and operator operands (§10.5) are also borrowed automatically.

### 11.4 Variadics and named collectors

- `migathavi xs: T role` must be the last fixed formal. Inside the callee `xs : &mukka[T]`. Zero elements are allowed.
- `migatha_perlu kw: jodi[padham, V]` receives labelled phrases with unknown labels, each with an unmarked role. There is at most one of each collector per function.

### 11.5 Evaluation order

Phrase operands are evaluated exactly once, left to right in source order, whichever formal they bind. Then defaults are materialized, then automatic receiver borrows are taken, then the body runs. Spreads are evaluated once and expanded in element order.

## 12. Pipes

12.1 `a |> stage` evaluates `a` once into a temporary `t`, then evaluates the stage with `t` **injected** into one formal:
- (a) **Hole form.** If the stage contains exactly one top-level `_` phrase, `t` takes that phrase's place, with its role and label. More than one `_`, or a `_` nested inside an operand, is E0213.
- (b) **Implicit form.** Otherwise `t` binds the **first declared non-collector formal**, which must not be bound by any other phrase of the stage (E0507). If the callee has only a `migathavi` collector, `t` becomes its first element.
- (c) A stage that is just a callee (`|> f()`) is the implicit form with no other phrases.

12.2 After injection, binding (§11.2) and automatic borrows (§11.3) apply as usual. The injected value is never evaluated again.

12.3 `|>` associates left and has the lowest precedence. `?` on a stage's callee (D11) propagates errors at that stage.

12.4 Pipes never spread: `xs |> f()` passes `xs` as a single value.

## 13. Traits and generics

### 13.1 Generic declarations

Functions, records, enums, methods blocks and implementations may take generic parameters `[T, U: Bound + Bound, N: ankam]`. Generic code is checked once against its bounds: inside a generic body, `T` supports only the operations its bounds provide. Generic code is **monomorphized**. Monomorphization must terminate (E0805).

### 13.2 Instantiation

At a call site, generic arguments are taken from explicit `type_args` or inferred by unifying formal types with argument types. When an intrinsic function (§21) takes only some type arguments explicitly, the rest are inferred.

### 13.3 Traits and implementations

```ante
gunam Choopinchochu ante:
    padham_ga(naa: &idhi) -> padham

Manishi ki Choopinchochu gunam undhi:
    padham_ga(naa: &idhi) -> padham ante:
        naa.peru.xerox() icchey
```

- A trait declares method signatures. A method with a body is a default.
- An implementation `Type ki Trait gunam undhi:` must define every method that has no default (E0802), with signatures matching after replacing `idhi` with `Type`.
- A marker trait is implemented with a bare line: `Point ki CopyKottochu gunam undhi`.
- **Coherence.** An implementation is allowed only if the trait or the type's outermost constructor is defined in the current package (E0803). Overlapping implementations are E0804. There is no specialization.
- Traits may take generic parameters (`gunam Okkokkati[I] ante:`). Associated types are deferred.
- Trait objects (dynamic dispatch) are deferred. All dispatch is static.

### 13.4 Methods

13.4.1 A function in a `panulu` block or implementation whose first parameter is `naa` is a **method**. The type of `naa` must be `idhi`, `&idhi` or `&maare idhi`. A function without `naa` is an **associated function**, called as `Type.f()`.

13.4.2 **Method lookup** for `recv.m`:
1. Auto-dereference `recv` (as for fields) to the candidate types. A reference is never a candidate itself: lookup moves straight to the type it points to, so `a.xerox()` with `a: &T` duplicates the `T`.
2. For each candidate in order, look first for an inherent method `m`, then for `m` among the visible traits implemented for that type.
3. The first candidate that has the method wins. Two traits supplying `m` at the same candidate is E0305.

13.4.3 **Receiver adjustment.**
- `&idhi` → automatic shared borrow.
- `&maare idhi` → automatic mutable borrow, which requires a mutable place (E0607).
- `idhi` → move (a copy for `CopyKottochu`).

The receiver borrow is taken **after** the argument phrases are evaluated, so `v.enni() ni v.cherchu()` is valid.

13.4.4 **Pro-drop of `naa`.** Inside a method body, an unqualified name `x` that isn't a local, parameter or module item resolves to `naa.x` when `idhi` has a field or method `x`. Any other ambiguity is E0305. Writing `naa.x` always works.

### 13.5 Standard traits

| Trait | Method(s) | Meaning |
|---|---|---|
| `CopyKottochu` | marker | "can be copy-kottu'd": a bitwise copy, implicit, like copying in an exam. Allowed only if every field is `CopyKottochu` and the type doesn't implement `Sardhukochu` (E0806) |
| `XeroxTheeyochu` | `xerox(naa: &idhi) -> idhi` | explicit duplicate; may allocate ("you pay for a xerox") |
| `Sardhukochu` | `sardhu(naa: &maare idhi) -> khali` | destructor ("pack up"), run on drop (§19.3) |
| `Polchochu` | `okateyna(naa: &idhi, itharam: &idhi ni) -> ato_ito` | equality, `==` ("are they the same?") |
| `VarusaPettochu` | `polchu(naa: &idhi, itharam: &idhi ni) -> Kramam` | total order, `<` etc. ("can be put in a line") |
| `HashCheyochu` | `hash_cheyyi(naa: &idhi, h: &maare Hasher ni) -> khali` | hashing; must agree with `Polchochu` |
| `Choopinchochu` | `padham_ga(naa: &idhi) -> padham` | display ("can be shown") |
| `Okkokkati[I]` | `tharvatha(naa: &maare idhi) -> undochu[I]` | iteration, "one by one" (`tharvatha` = "next") |
| `Kalupochu`, `Theeseyochu`, `Guninchochu`, `Panchochu`, `Sheshamochu` | `kalupu`/`theesey`/`guninchu`/`panchu`/`shesham(naa: &idhi, itharam: &idhi ni) -> idhi` | `+ - * / %` |
| `Thippochu` | `thippu(naa: &idhi) -> idhi` | unary `-` ("flip") |
| `MatmulCheyochu` | `matmul(naa: &idhi, itharam: &idhi ni) -> idhi` | `@` |
| `NunchiRavochu[F]` | `nunchi(vishayam: F ni) -> idhi` | conversion "can come from F"; used by `?` (§10.6) |

`Hasher` is a prelude record with `x ni h.kalupu()` for every integer type, `aksharam`, `ato_ito` and `&mukka[aasajeevi ankam8]`, and `h.mugimpu() -> aasajeevi ankam64` ("finish"). The algorithm is implementation-defined but must be deterministic for a fixed seed.

`Kramam` is `edho_okati Kramam ante: Thakkuva, Samaanam, Ekkuva` (less, equal, more).

**Built-in implementations:**
- Numeric types, `ato_ito`, `aksharam` and `khali` implement `CopyKottochu`, `XeroxTheeyochu`, `Polchochu` and `Choopinchochu`.
- Integers, `ato_ito`, `aksharam` and `khali` also implement `VarusaPettochu` and `HashCheyochu`.
- Integers and floats implement `Kalupochu`, `Theeseyochu`, `Guninchochu`, `Panchochu` and `Sheshamochu`; signed integers and floats also implement `Thippochu`, and `padham` implements `Kalupochu`. Generic code bounded by these traits can therefore use the operators on numbers.
- **Floats implement neither `VarusaPettochu` nor `HashCheyochu`.** Their `<` and `==` are built-in IEEE operations (§18.3).
- `padham` implements everything except `CopyKottochu`.
- `jatha`, `[T;N]`, `varusa`, `undochu`, `sare_thappu`, `jodi` and `pette` implement each trait whenever their components do. The exceptions: `jodi` has no `VarusaPettochu`/`HashCheyochu`, and only the types marked in §9.1 have `CopyKottochu`.

### 13.6 `@automatic`

`@automatic Polchochu, XeroxTheeyochu, …` before a `rakam` or `edho_okati` derives the listed traits field by field, in declaration order. Derivable traits: `CopyKottochu`, `XeroxTheeyochu`, `Polchochu`, `VarusaPettochu`, `HashCheyochu` and `Choopinchochu`. The derived `Choopinchochu` prints `Name{f: v, …}` / `Enum.Variant(v, …)`. A component lacking the trait is E0807.

## 14. Patterns and exhaustiveness

14.1 **Irrefutable patterns** are required in bindings and `for` loops (E0703): bindings, `_`, tuples of irrefutable patterns, and records of irrefutable patterns.

14.2 **Binding mode.**
- Matching a value binds by move (a copy for `CopyKottochu`).
- Matching through `&T`: a binding whose type is known to be `CopyKottochu` (concrete or bounded) binds by copy; any other binding binds as `&U`.
- The same applies through `&maare T`, where non-`CopyKottochu` bindings get `&maare U`.

14.3 **Exhaustiveness.** A `batti` must be exhaustive for enums, `undochu`, `sare_thappu`, `ato_ito` and tuples of these, computed with the standard usefulness algorithm (E0701 "`X` case marchipoyav!"). For integers, strings and characters, exhaustiveness requires `_` or a binding arm. An unreachable arm is warning W0004.

14.4 Arms are tried in textual order. The first match wins, and there is no fallthrough.

## 15. Ownership and borrowing

### 15.1 Moves and copies

15.1.1 Evaluating a **place** of non-`CopyKottochu` type in a value context **moves** it. Value contexts are: a binding initializer, a call operand bound to a non-reference formal, an `icchey` operand, a constructor field, an assignment right-hand side, and an operator operand of a non-`CopyKottochu` type passed by value. A `CopyKottochu` place is copied instead.

15.1.2 **Allowed moves:**
- out of a local;
- out of a field of a local that is owned all the way down (a partial move; the local can't be used as a whole afterwards, but other fields can);
- out of `pette` via `*b`.

Moving out of an index place or through a reference is E0603. Use `.xerox()`, `undochu` methods, or `varusa.theesey()` instead.

15.1.3 **Initialization analysis.** The compiler tracks, per place and per program point, whether it is *initialized*, *moved* or *maybe-moved*. Reading or borrowing a place that isn't definitely initialized is E0601 ("`a` ni already icchesav kada!") or E0609. Assigning to a moved local re-initializes it.

### 15.2 Places

A **place** is a local, a field of a place, an index of a place, a `*` of a reference/`pette`/raw pointer, or a slice of a place.

### 15.3 Mutability

A place is **mutable** if any of the following holds:
- its root local is `maare`;
- it is reached through a `&maare` dereference;
- it is the `naa` parameter of type `&maare idhi`.

Assignment, compound assignment, `&maare` and calls requiring a mutable receiver need a mutable place (E0602 "`x` maare kaadhu, maarchalevu"). Record fields have no mutability of their own.

### 15.4 Loans and conflicts

The borrow checker works on the MIR control-flow graph (Appendix E). It is a non-lexical lifetime analysis:

15.4.1 Each `&p` creates a **shared loan** of `p`, and each `&maare p` creates a **mutable loan** of `p`. Automatic borrows (§11.3, §13.4) also create loans.

15.4.2 A loan is **live** at a program point if some value derived from the borrow may still be used later. A derived value is the reference itself, any RC value computed from it, or a result returned under §9.3.2(c) from a call that received it. Liveness is the usual backward dataflow over the MIR.

15.4.3 Two places **overlap** if one is a prefix of the other. All index and slice places of the same base overlap each other.

15.4.4 At each point, for each live loan L of place p, an access to an overlapping place q is an error when:

| Access to q | Conflicts with a live shared loan | Conflicts with a live mutable loan |
|---|---|---|
| read or copy | no | yes, unless done through L |
| move | yes (E0604) | yes (E0605) |
| write | yes (E0604) | yes, unless done through L |
| `&q` | no | yes (E0605) |
| `&maare q` | yes (E0604) | yes (E0605) |
| `varusa` operations that may reallocate (`cherchu`, `theesey`, …) | count as a write to the whole vector | count as a write to the whole vector |

15.4.5 A reference must not outlive its referent. Dropping or moving the owner while a loan of it is live is E0606.

15.4.6 **Closures.** Creating a closure takes its capture loans, which stay live as long as the closure value is live.

### 15.5 Temporaries

Temporaries are dropped at the end of the enclosing statement, in reverse order of creation. **Exception:** if a binding's initializer is exactly `&e` or `&maare e` and `e` is a temporary, that temporary lives until the end of the enclosing block.

## 16. Constant evaluation

16.1 These must be constant expressions (E1101): module constants, array lengths, const generic arguments and default values.

16.2 A constant expression is built only from:
- literals;
- other constants;
- arithmetic, bitwise and comparison operators on built-in scalars;
- tuple, array and record literals of constant parts;
- `maarchu`/`kolpothu_maarchu` between scalar types;
- `padham` literals.

There are no function calls in 1.0 constant expressions.

16.3 Constant evaluation uses the semantics of Part V. A trap during constant evaluation is a compile-time error E1102 with the trap reason. Implementations must allow at least 1,000,000 evaluation steps; exceeding their documented limit is E1103.

---

# Part V — Dynamic semantics

## 17. Values and evaluation

17.1 **Representation.** Except for `@C` records (§22.3) and scalars at the FFI boundary, the layout of values is implementation-defined and unobservable in safe code.

17.2 **Evaluation order.**
- Every expression is evaluated exactly once, left to right.
- Binary operators: left, then right. `&&`/`||` short-circuit.
- Indexing: base, then index (or base, lower bound, upper bound).
- Calls: §11.5. Pipes: §12.
- Assignment: the place's subexpressions, then the right-hand side, then the write. The old value is dropped after the write.
- Record and tuple literals: fields in **source order**.
- f-strings: interpolations left to right.

17.3 **Moves** are bitwise transfers. The source becomes uninitialized and is not dropped.

## 18. Arithmetic

### 18.1 Integers

- `+ - *` and unary `-` **trap** on overflow (T01) in every build profile.
- `/` truncates toward zero. `%` takes the sign of the dividend, so `a == (a / b) * b + a % b`. Division or remainder by zero traps (T02). `MIN / -1` and `MIN % -1` trap (T01).
- `<<` and `>>` trap if the count is negative or ≥ the bit width (T04). `>>` is arithmetic for signed types and logical for unsigned ones. `<<` discards bits shifted out and never traps on value overflow.
- `& | ^ ~` are bitwise on the two's-complement representation.

### 18.2 Explicit arithmetic families (`lekkalu`)

For every integer type and op ∈ {`kalupu` (+), `theesey` (−), `guninchu` (×)}:

| Function | Result |
|---|---|
| `a ni, b tho lekkalu.chuttu_op()` | wraps around modulo 2ᴺ ("around") |
| `a ni, b tho lekkalu.trupthiga_op()` | saturates at the type's min/max ("contentedly": it stops when it's had enough) |
| `a ni, b tho lekkalu.kudhirithe_op()` | `undochu[T]`: `ledhu` on overflow ("if it works out") |

`lekkalu.kudhirithe_panchu` is checked division, returning `ledhu` for division by zero or `MIN / -1`. Implementations must compile `chuttu_*` and `trupthiga_*` to single instructions where the target has them.

### 18.3 Floating point

- IEEE 754 binary32/binary64 with round-to-nearest, ties-to-even.
- `NaN != NaN` is `avunu`; every other comparison involving NaN is `kaadhu`. `+0.0 == -0.0`.
- `%` is IEEE `fmod`. Division by zero produces ±Inf or NaN and doesn't trap.
- Outside `@vegam`, implementations must not reassociate, contract into FMA, or assume the absence of NaN/Inf. Results are bit-reproducible for a given target, except for transcendental functions in `lekkalu`, whose accuracy is documented per implementation.
- **`@vegam` ("speed")** on a function allows, for `amsam` operations written **lexically inside that function**, reassociation (including reordering reductions) and FMA contraction. It doesn't allow assuming NaN or Inf are absent, doesn't affect integer semantics, and doesn't propagate into called functions. In LLVM terms: `reassoc` and `contract` only.

### 18.4 Conversions

| Function | Allowed source → target | Result |
|---|---|---|
| `v ni maarchu[T]()` | **lossless only**: integer → wider integer of the same signedness; unsigned → strictly wider signed; integer with ≤ 24 bits → `amsam32` and ≤ 53 bits → `amsam64`; `amsam32` → `amsam64`; `aksharam` → `aasajeevi ankam32` | `T`. A pair that isn't lossless is a **compile error** E0409 that suggests the two functions below |
| `v ni kudhirithe_maarchu[T]()` | any numeric pair; `aasajeevi ankam32` → `aksharam` | `undochu[T]`: `ledhu` if the value isn't exactly representable (or isn't a valid scalar for `aksharam`) |
| `v ni kolpothu_maarchu[T]()` | any numeric pair | always succeeds, losing information on purpose ("while losing"). Integer → integer truncates the two's complement. Float → integer rounds toward zero, saturates at min/max, and NaN → 0. Integer → float rounds to nearest. `amsam64` → `amsam32` rounds to nearest |

## 19. Control flow and drop

19.1 **Loops.**
- `aapey` ("stop it") leaves the innermost loop.
- `vadhiley` ("leave it") skips to the next iteration.
- `c unnanthasepu:` ("as long as") re-evaluates `c` before each iteration. `avunu unnanthasepu:` loops forever.
- A `for` loop evaluates its iterable once, then calls the iterator's `tharvatha` until `ledhu`.

19.2 **Ranges.** `a nunchi b lopu` yields a, a+1, …, b−1. `a nunchi b varaku` yields a, …, b. Both bounds are evaluated once, and an empty range is fine. Iteration must not overflow: an inclusive range ending at the type's maximum stops after yielding it.

19.3 **Drop.** When a scope ends, by falling through, `icchey`, `aapey`, `vadhiley`, or `?` propagation, its initialized locals are dropped in reverse order of initialization. A maybe-moved local carries a runtime drop flag.

Dropping a value:
1. runs `Sardhukochu.sardhu` if the type implements it;
2. then drops the fields in reverse declaration order (and elements of collections from first to last).

The drop order of collection elements is implementation-defined, but it is deterministic.

19.4 **`vellagane:`** ("as soon as we leave"). Running a `vellagane:` statement registers its block. Registered blocks run in LIFO order when the enclosing block exits normally (fall-through, `icchey`, `aapey`, `vadhiley`, `?`), **before** the locals of that block are dropped. They don't run on a trap. A `vellagane` block may not contain `icchey`, `aapey`, `vadhiley` or `?` (E0214).

19.5 **`?`.** On `thappu(e)`, `?` runs the cleanup of §19.3 and §19.4 for every scope it leaves, then returns `thappu(e)`. On `sare(v)` the result is `v`.

## 20. Program execution and traps

20.1 **Startup.** The runtime initializes, then calls `modhalu()`.

20.2 **Exit status.**

| Outcome | Exit status | Output |
|---|---|---|
| `modhalu` returns `()` or `sare(())` | 0 | — |
| `modhalu` returns `thappu(e)` | 1 | `thappu: <e.padham_ga()>` on stderr |
| `system.vellipo(n)` ("go away") | `n` | no locals are dropped |
| any trap | 101 | trap message on stderr |

20.3 **Traps** end the process. There is no unwinding and no catching, and cleanup isn't guaranteed. The message format is:

```text
kompa munigindhi! <reason> (<file>:<line>:<col>)
```

| Code | Reason |
|---|---|
| T01 | integer overflow, e.g. `ankam8 lo 127 + 1 pattadhu` ("doesn't fit") |
| T02 | division by zero |
| T03 | index out of bounds (index, length) |
| T04 | invalid shift count |
| T05 | explicit `kompa_munigindhi()` with its message |
| T06 | `kachithanga()` on `ledhu`/`thappu` |
| T07 | `nijame()` assertion failed, with its message |
| T08 | stack overflow |
| T09 | out of memory |
| T10 | invalid slice bounds (a > b, or b > length) |

20.4 Output to stdout is buffered and flushed at normal exit, before a trap message, and by `system.flush()`.

20.5 **Determinism.** Apart from I/O, time, environment, `HashCheyochu` seeds (which may be randomized per process, though `jodi` iteration stays insertion-ordered), and documented `@vegam` effects, program behavior is deterministic.

20.6 **Implementation-defined** behavior (to be documented): value layout (except §22.3), stack size, accuracy of transcendental functions, the drop order of collection elements, hash seeds, the constant-evaluation step limit, and the text of diagnostics.

---

# Part VI — Standard library

## 21. Prelude and core modules

Signatures are normative; the descriptions are summaries. Functions marked †intrinsic are built into the compiler and may be generic in ways user code can't express (for example, over all integer types or all iterables).

### 21.1 Prelude functions

| Signature | Behavior |
|---|---|
| `choopinchu[T: Choopinchochu](vishayam: &T ni) -> khali` | writes `padham_ga()` plus `\n` to stdout ("show") |
| `cheppu[T: Choopinchochu](vishayam: &T ni) -> khali` | the same, without a newline ("say") |
| `gola_pettu[T: Choopinchochu](vishayam: &T ni) -> khali` | writes to stderr with a newline ("make a fuss") |
| `nijame(sharathu: ato_ito, sandesham: padham ni = "") -> khali` | trap T07 if `kaadhu` ("it's true!"). Always enabled. Usage: `(x > 0) nijame()` |
| `kompa_munigindhi(sandesham: padham ni = "") -> raadhu` | trap T05 ("the house has sunk") |
| `maarchu[T]`, `kudhirithe_maarchu[T]`, `kolpothu_maarchu[T]` | †intrinsic; §18.4 |

### 21.2 Built-in type methods

**`padham`**
- `enni() -> ankam`: scalar count ("how many?")
- `byte_len() -> ankam`
- `aksharaalu()`: iterator of `aksharam`
- `sep ni s.vidagottu() -> varusa[padham]`: split ("break apart")
- `kathirinchu() -> padham`: trim ("cut")
- `p ni s.tho_modhalaindha() -> ato_ito`: starts with
- `p ni s.lo_undha() -> ato_ito`: contains
- `ankam_ga(base: ankam32 ni = 10) -> sare_thappu[ankam, ParseThappu]`, `aasajeevi_ga(base: ankam32 ni = 10) -> sare_thappu[aasajeevi ankam, ParseThappu]`. Usage: `16 ni base ga s.ankam_ga()`
- `amsam_ga() -> sare_thappu[amsam, ParseThappu]`, which must round correctly (exact IEEE nearest)
- `baitlu() -> &mukka[aasajeevi ankam8]`
- `padham.baitlu_nunchi(b: &mukka[aasajeevi ankam8] ni) -> sare_thappu[padham, Utf8Thappu]`
- `a ni, b tho s.byte_bhaagam() -> sare_thappu[padham, Utf8Thappu]`: copies bytes `a` up to (not including) `b`; `thappu` if a bound isn't on a character boundary
- `xerox()`, `+`, `==`, `<`

**`varusa[T]`**
- `varusa.kotthadhi() -> varusa[T]`: "a new one"
- `enni() -> ankam`
- `khaaliyaa() -> ato_ito`: "is it empty?"
- `x ni v.cherchu()`: append ("add in")
- `v.theesey() -> undochu[T]`: pop ("remove")
- `i ni v.thechu() -> undochu[&T]`: get ("bring")
- `i ni v.maare_thechu() -> undochu[&maare T]`
- `v.sort_cheyyi()` (needs `T: VarusaPettochu`); all sorts are **stable**
- `f ni v.ila_sort_cheyyi()` with `f: pani[(&T ni, &T tho) -> Kramam]`: sort "like this"
- `v.xerox()`
- indexing and slicing

**`mukka[T]`**: `enni`, `khaaliyaa`, `thechu`, indexing and slicing.

**`jodi[K: Polchochu + HashCheyochu, V]`**
- `jodi.kotthadhi()`
- `k ni, v tho m.cherchu() -> undochu[V]`: insert, returning the old value; a replaced key keeps its position
- `k ni m.thechu() -> undochu[&V]`, `k ni m.maare_thechu() -> undochu[&maare V]`
- `k ni, d tho m.lekapothe_cherchu() -> &maare V`: inserts `d` only if `k` is missing, then returns the entry
- `k ni m.theesey() -> undochu[V]`
- `k ni m.undhaa() -> ato_ito`: "is it there?"
- `enni`
- iteration yields `jatha[&K, &V]` in insertion order

**`gajibiji_jodi[K, V]`** ("jumbled map"): the same API without any order guarantee. **`gumpu[T]`** ("group"): a set, with `cherchu`, `undhaa`, `theesey` and `enni`.

**`undochu[T]`**
- `undhaa()`, `ledhaa()`
- `d ni o.lekapothe() -> T`: "otherwise"
- `kachithanga() -> T`: "definitely"; trap T06 on `ledhu`
- `f ni o.prathi_dhaaniki() -> undochu[U]`: map

**`sare_thappu[T, E]`**
- `sareyaa()`
- `kachithanga()`: trap T06 on `thappu`
- `f ni r.thappu_maarchu() -> sare_thappu[T, F]`: map the error
- `f ni r.prathi_dhaaniki() -> sare_thappu[U, E]`: map the value

**`pette[T]`**: `x ni pette.kotthadhi()`; `*b` moves out; `.` auto-dereferences.

**`aksharam`**: `padham_ga()`, the `maarchu` conversions, and the predicates `digit_aa()`, `letter_aa()`, `space_aa()`, `alnum_aa()` (Unicode properties) and `ascii_aa()`.

**Numbers**: `x.hex_ga() -> padham` for integers; `x.bits() -> aasajeevi ankam64/32` and `amsam64.bits_nunchi(b ni)` / `amsam32.bits_nunchi(b ni)` for exact float bit patterns; `size_of[T]()`, `align_of[T]()` †intrinsic.

**`Raatha`** ("writing"): a growable string builder. `Raatha.kotthadhi()`; `x ni r.raayi()` appends any `Choopinchochu` value without an intermediate `padham`; `c ni r.akshara_raayi()`; `r.enni()`; `r.padham_ga() -> padham` (moves the contents out, leaving the builder empty). Appending is amortized O(1).

### 21.3 Modules

| Module | Contents |
|---|---|
| `lekkalu` (math) | the §18.2 families; `sqrt sin cos tan exp log abs min max pow floor ceil round`; `PI E INF NAN`; `lekkalu.motham(xs)` (sum) and `a ni, b tho lekkalu.dot()` over `&mukka[amsam32/64]` using a documented blocked order (vectorizable without `@vegam`); `lekkalu.polchu_amsam(a, b) -> Kramam` (IEEE totalOrder) |
| `file` | `p ni file.chadhuvu() -> sare_thappu[padham, FileThappu]` ("read"); `p ki, text ni file.raayi() -> sare_thappu[khali, FileThappu]` ("write"); `p ni file.baitlu_chadhuvu()`, `p ki, b ni file.baitlu_raayi()`; `p ni file.undhaa() -> ato_ito`; `p ni file.folderaa() -> ato_ito`; `p ni file.list_cheyyi() -> sare_thappu[varusa[padham], FileThappu]` (sorted names); `p ni file.folder_cheyyi()` (create, including parents); `p ni file.theesey()` (delete a file); `a ni, b tho file.path_kalupu() -> padham`; `p ni file.path_peru()`, `p ni file.path_folder()`, `p ni file.path_extension() -> undochu[padham]` |
| `system` | `system.args() -> varusa[padham]`; `k ni system.env() -> undochu[padham]`; `system.line_chadhuvu() -> undochu[padham]` (stdin); `system.flush()`; `n ni system.vellipo() -> raadhu` (exit); `prog ni, args tho system.nadupu() -> sare_thappu[Phalitham, SystemThappu]` ("run": waits for the program, where `Phalitham{status: ankam32, stdout: padham, stderr: padham}`); `system.os() -> padham`, `system.arch() -> padham` |
| `memory` | †intrinsic, **`naa_risk` only**: `n ni, a tho memory.allocate() -> *maare khali`, `p ni, n tho, a tho memory.release()`, `p ni, n tho, a tho, m ki memory.reallocate()`, `src ni, dst ki, n tho memory.copy()`. Used to write runtime and collection code in "Ante" itself |
| `samayam` (time) | `samayam.ippudu() -> Kshanam` (monotonic "now"); `k.gadichindhi() -> Kaalam` ("elapsed"); `Kaalam.millisec()`, `.nanosec()` |

`FileThappu` and `ParseThappu` are enums that implement `Choopinchochu`.

### 21.4 Iteration

21.4.1 **Iterables:**
- ranges (`Paridhi[I]`, with `I` integer; "extent");
- `&[T;N]`, `&varusa[T]`, `&mukka[T]`: yield `T` if `T` is known `CopyKottochu`, else `&T`;
- the `&maare` versions: yield `&maare T`;
- an owned `varusa[T]` temporary or `xs.anni_theesuko()`: yields `T` by move ("take all");
- `&jodi[K,V]`, `&gumpu[T]`;
- `padham.aksharaalu()`;
- any type implementing `Okkokkati[I]`;
- the adapters below.

A `for` over a place of collection type borrows it shared.

21.4.2 **Adapters** (†intrinsic; lazy; RC when built on borrowed data):

| Adapter | Meaning |
|---|---|
| `a, b kalipi()` | zip, yielding `jatha[A, B]` ("together") |
| `xs, n ni modhati()` | the first `n` |
| `xs, n ni dhaatu()` | skip `n` ("jump over") |
| `xs, f ni prathi_dhaaniki()` | map ("to each one") |
| `xs, f ni jalleda_pattu()` | filter ("put it through a sieve"); `f` receives `&I` |
| `xs, init ni, f tho poguchey()` | fold ("gather up") |
| `xs.number_vesi()` | enumerate, yielding `jatha[ankam, I]` |
| `xs.varusa_ga()` | collect into `varusa[I]` |

Adapters work with pipes: `xs |> 10 ni modhati() |> varusa_ga()`.

### 21.5 Deferred to later editions

Threads and concurrency (`thread`, `Pampochu`, `Panchukovochu`, locks `Thaalam`, channels, atomics), networking, `kitiki` strided views, `panchukunna` reference counting, and `baitlu` as a separate buffer type. See Appendix F.

---

# Part VII — Interop

## 22. C FFI and `naa_risk`

22.1 **Importing C functions.**

```ante
bayata "C" ante:
    sqrt(x: amsam64) -> amsam64
    malloc(n: aasajeevi ankam64) -> *maare aasajeevi ankam8
```

- Parameters have no roles. They bind in order, as unmarked formals.
- FFI-safe types are integers, floats, `khali`, raw pointers and `@C` records (E1002 otherwise). `*khali` is C's `void *`, used for opaque handles such as `LLVMModuleRef`.
- Calling a `bayata` function is an unsafe operation.

22.2 **`naa_risk:`** ("my risk"). The following operations are allowed only inside a `naa_risk:` block (E1001):
- calling `bayata` functions;
- dereferencing raw pointers;
- pointer arithmetic (`n ni p.jarugu()`, "shift over");
- `kolpothu_maarchu` between pointers and integers;
- `CopyKottochu` implemented on a type with raw-pointer fields.

The programmer guarantees alignment, validity, lifetime, aliasing and the calling convention. A violation is undefined behavior. Getting a raw pointer is safe: `&x` coerces to `*T` where a raw pointer is expected, and `s.pointer()` works on `mukka`.

22.3 **`@C` records** have the target's C layout, with fields in declaration order.

22.4 **Exporting.** `@bayata "name"` before a function exports it with the C ABI under that symbol. Its parameters and result must be FFI-safe. Roles are ignored in the C signature. A trap inside an exported function aborts the process, and never unwinds into C.

22.5 Internal types (`padham`, `varusa`, `jodi`, `undochu`, `sare_thappu`, `pani`) must not cross the C ABI directly. Pass pointers and lengths or opaque handles instead.

---

# Part VIII — Tooling

## 23. `antec`, manifest, tests and diagnostics

23.1 **Commands.**

| Command | Action |
|---|---|
| `antec build [--release]` | compile the package |
| `antec run [-- args]` | build, then run |
| `antec pariksha` | build and run tests ("exam") |
| `antec fmt` | canonical formatting; must preserve the AST and evaluation order |
| `antec check [path]` | check a file, or a package folder (default: the current folder), without generating code |
| `antec migrate` | rewrite code for renamed words and new editions (§23.5) |
| `antec emit (tokens\|ast\|mir\|llvm)` | debugging output |

`--release` changes optimization level only, never semantics.

23.2 **`ante.toml`.**

```toml
[package]
name = "naa_project"
version = "0.1.0"
edition = "1.0"
kind = "bin"            # or "lib"

[dependencies]
lekka_pustakam = "1.2"

[features]
torch = false           # Annex T
python = false          # Annex P
```

A lockfile `ante.lock` pins exact versions, and builds with the lockfile are reproducible. An optional `[build]` table sets `stack = "64MB"` (main-thread stack size; the default is at least 8 MB) and `target = "<llvm triple>"`.

23.2.1 **Installation layout.** A toolchain installation is a directory `$ANTE_HOME` (default: the directory above the `antec` binary) with:

```text
$ANTE_HOME/bin/antec
$ANTE_HOME/lib/ante/std/<module>.ante      # standard library, shipped as source
$ANTE_HOME/lib/ante/runtime/<target>/      # prebuilt runtime objects
```

The standard library is ordinary "Ante" source compiled together with each program. Only the small runtime shim (traps, entry point, OS calls) is prebuilt. A self-hosted compiler uses the same layout.

23.2.2 **Determinism.** For identical inputs, flags, toolchain and target, `antec` must produce byte-identical outputs. A compiler written in "Ante" must therefore never let `gajibiji_jodi` iteration order or hash seeds affect its output.

23.3 **Tests.** `@pariksha` marks a function `name() -> khali` or `name() -> sare_thappu[khali, E]` as a test. Each test runs as if in its own process. It passes if it returns normally (with `sare` for the second form), and fails on a trap or `thappu`. `nijame` is the assertion. Tests are compiled with the same semantics as release code.

23.4 **Diagnostics.** The format is:

```text
error[E0601]: `a` ni already `b` ki icchesav kada! Malli vaadalevu.
  --> src/modhalu.ante:4:5
```

23.5 **Editions and vocabulary evolution.**
- `ante.toml`'s `edition` selects the edition. One compiler supports several editions, and packages of different editions link together.
- Editions differ only in surface syntax and vocabulary. They all lower to the same AST/HIR, so type checking, borrow checking and code generation don't depend on the edition.
- While Edition 1.0 is a draft (`antec` 0.x), a renamed word stays accepted as a deprecated alias with warning W0100 for one minor release, and is then removed.
- Once `antec` 1.0.0 ships, Edition 1.0 is frozen: a program valid under it stays valid under every later compiler. Further vocabulary changes go into Edition 2.0.

23.6 **Diagnostics format.** `--message-format=json` emits `{code, severity, file, line, col, end_line, end_col, message}`. Lines and columns are 1-based. A column counts Unicode scalar values from the start of the line. Conformance tests compare only `code` and the primary span. Codes are listed in Appendix B.

---

# Annex T — PyTorch binding ("Ante" Torch 1.0)

T.1 **Import.** `torch thechuko` links LibTorch through a generated C shim over ATen (the same approach as Rust's `tch` crate). It doesn't require CPython. It requires `features.torch = true`.

T.2 **Types.**
- `torch.Tensor` is an opaque reference-counted handle. It does **not** implement `CopyKottochu`.
- `.xerox()` makes a handle copy that shares storage. `.clone()` (PyTorch's own name) copies the data.
- `torch.nn.Linear`, other modules and `torch.optim.*` are concrete opaque handle types. Dispatch happens inside LibTorch, so no language-level trait objects are needed.

T.3 **Operators.** `torch.Tensor` implements `Kalupochu`, `Theeseyochu`, `Guninchochu`, `Panchochu`, `Thippochu` and `MatmulCheyochu`, with PyTorch broadcasting. Comparison operators return tensors through named methods (`a ni b.eq()`), not `==`.

T.4 **Default dtype** for constructors is `amsam32`.

T.5 **Indexing.** Tensors use the multi-index form `x[i, j, …]`. Each position takes an integer (negatives count from the end), a range `a nunchi b lopu`, or `_` for the whole dimension. The result is a view tensor.

T.6 **Arguments.** PyTorch's positional parameters become `ni`-role formals in PyTorch's declaration order. Keyword parameters become named formals supplied with `ga`. Example: `3 ni, 4 ni torch.randn()`, `torch.float32 ni dtype ga`.

T.7 **Errors.**
- Operations that fail only through programmer error (shape, dtype or device mismatch) trap with T05 and the LibTorch message.
- Checked variants live in `torch.choosi` ("after checking") and return `sare_thappu[torch.Tensor, TorchThappu]`.
- I/O, loading and device setup always return `sare_thappu`.

T.8 **Interchange.**
- `t.mukka_ga() -> sare_thappu[&mukka[T], TorchThappu]` borrows CPU, contiguous storage, and is RC.
- `xs ni torch.mukka_nunchi()` copies data in.
- DLPack exchange is supported. Zero-copy is conditional.

T.9 **Autograd, CUDA and MPS** are available as in PyTorch (`loss.backward()`, `"cuda" ni t.ki_pampu()`).

# Annex P — Python bridge ("Ante" Python 1.0)

P.1 **Import.** `python nunchi pkg thechuko` embeds CPython. It requires `features.python = true`. Programs without such an import never link CPython.

P.2 **Dynamic values** have type `edhaina` ("anything"), an opaque reference-counted handle that manages the GIL and reference counts.

P.3 **Role binding.** Python positional parameters bind as `ni` formals in declaration order. Keyword parameters bind with `ga`. Positional-only and keyword-only constraints are honored (E0505 for keyword-only misuse).

P.4 **Stubs.** The toolchain generates "Ante" declarations from `.pyi` stubs. Known types are statically typed. Everything else is `edhaina`.

P.5 **Errors.** Every bridge call returns `sare_thappu[T, PythonThappu]`. Python exceptions are caught and converted, and never unwind through "Ante" frames.

---

# Appendix A — Vocabulary and romanization

## A.1 Principles

| Rule | Meaning |
|---|---|
| R1 | Use spoken register, not newspaper register |
| R2 | English technical nouns, Telugu grammar (`file`, `system`, `torch.Tensor`, `thread`) |
| R3 | An English stem with a Telugu suffix is legitimate Tinglish (`HashCheyochu`, `CopyKottochu`, `sort_cheyyi`) |
| R4 | Use the grammatical form the slot needs: a participle for a modifier (`maare`), the genitive for a possessive (`naa`), `-ochu` for a capability |
| R5 | A pun earns its place only if it teaches the meaning (`aasajeevi`, `CopyKottochu`/`xerox`, `trupthiga_kalupu`, `kolpothu_maarchu`, `naa_risk`, `jalleda_pattu`) |
| R6 | One spelling per word, under rule A.2 |

## A.2 Romanization

| Sound | Spelling | Examples |
|---|---|---|
| Dental త / ద | `th` / `dh` | `jatha`, `padham`, `thappu`, `ledhu`, `undhi`, `vadhiley`, `thechuko` |
| Retroflex ట / డ | `t` / `d` | `ante`, `pettu`, `batti`, `undochu` |
| Geminate consonants | doubled letters | `mukka`, `icchey`, `kotthadhi` |
| Aspirates | `h` only for a true aspirate (ఫ = `ph`) | never `chh` for చ్చ |
| Long vowels | `aa`, `ee`, `oo` where they distinguish words | `maare`, `maarchu`, `aapey`, `theesuko` |

All keywords and standard-library names in this spec follow A.2. The locked words `ankam`, `aasajeevi`, `amsam`, `ato_ito`, `padham`, `jatha`, `jodi` and `undochu` are unchanged.

## A.3 Glossary

| Word | Literal meaning | Use |
|---|---|---|
| `ante` | means | definitions; the language's name |
| `aithe` / `ledhante` | if so / otherwise | conditionals |
| `unnanthasepu` | as long as | while |
| `okko … ki` | to each one | for |
| `batti` | depending on | match |
| `pettu` / `icchey` | put / hand over | assignment / return |
| `maare` | the changing one | mutable |
| `naa` / `idhi` | my / this | receiver / Self type |
| `thechuko` | bring for yourself | import |
| `aapey` / `vadhiley` | stop it / leave it | break / continue |
| `vellagane` | as soon as we leave | defer |
| `naa_risk` | my risk | unsafe |
| `andhariki` | for everyone | public |
| `theesuko` | take it | move closure |
| `rakam` / `edho_okati` | kind / one of these | record / enum |
| `gunam` / `panulu` | quality / tasks | trait / methods |
| `undhi` / `ledhu` | it's there / it's not | optional |
| `sare` / `thappu` | okay / wrong | result |
| `khali` / `raadhu` | empty / won't come | unit / never |
| `varusa` / `mukka` / `pette` | row / piece / box | vector / slice / box |
| `pani` | task | function type |
| `kompa_munigindhi` | the house has sunk | panic |
| `nijame` | it's true! | assert |
| `trupthiga_kalupu` | add contentedly | saturating add |
| `chuttu_kalupu` | add around | wrapping add |
| `kudhirithe_…` | if it works out | checked operation |
| `kolpothu_maarchu` | change while losing | lossy conversion |
| `lekkalu` | sums, calculations | math module |
| `jalleda_pattu` | put through a sieve | filter |
| `@vegam` | speed | relaxed floating point |
| `@pariksha` | exam | test |

---

# Appendix B — Diagnostic codes

Severity is error unless the code starts with W. The prose shown is the reference wording and is not normative.

| Code | Condition | Reference prose |
|---|---|---|
| **Lexical** | | |
| E0101 | invalid UTF-8 or non-ASCII outside literals | |
| E0102 | tab in indentation | |
| E0103 | dedent doesn't match any outer level | |
| E0104 | unterminated string | |
| E0105 | unknown escape | |
| E0106 | malformed number | |
| E0108 | invalid `\u{…}` | |
| E0109 | character literal without exactly one character | |
| E0110 | malformed f-string braces | "`}` ni `}}` ani raayi" |
| **Syntax** | | |
| E0201 | unexpected token | |
| E0202 | phrases with no callee | "verb ekkada?" |
| E0203 | missing comma between phrases | |
| E0204 | `f(args)` call form | "`args ni f()` ani raayi" |
| E0205 | misplaced `ga` | |
| E0206 | `;` outside an array type | |
| E0207 | `ledhante` without `aithe` | |
| E0208 | annotation not followed by a declaration | |
| E0209 | executable statement at module level | |
| E0210 | compound assignment used as an expression | |
| E0211 | nested function declaration | |
| E0212 | invalid pipe stage | |
| E0213 | pipe hole misuse, or `aapey`/`vadhiley` outside a loop | |
| E0214 | control flow inside `vellagane` | |
| E0215 | invalid assignment target | |
| **Names** | | |
| E0301 | undefined name | |
| E0302 | duplicate definition | |
| E0303 | private item | |
| E0304 | import of an unknown item | |
| E0305 | ambiguous method | |
| E0306 | use before definition | |
| E0307 | keyword used as an identifier | |
| E0308 | redefinition in the same scope | |
| **Types** | | |
| E0401 | type mismatch | |
| E0402 | literal out of range | |
| E0403 | non-`ato_ito` condition | |
| E0404 | type annotation needed | "type cheppu" |
| E0405 | no such field | |
| E0406 | no such method | |
| E0407 | operator not implemented | |
| E0408 | mixed signedness | |
| E0409 | lossy `maarchu` | "`kolpothu_maarchu` / `kudhirithe_maarchu` vaadu" |
| E0410 | missing `icchey` on some path | |
| E0411 | `?` outside a `sare_thappu` function | |
| E0412 | `?` error type mismatch | |
| E0413 | indexing a `padham` | |
| E0414 | non-integer index | |
| E0415 | RC type stored | |
| E0416 | record literal fields wrong | |
| E0417 | chained comparison | |
| E0418 | infinitely sized type | |
| E0419 | duplicate constant map key | |
| **Calls** | | |
| E0501 | missing argument | "`f` ki `tho` evaru ivvaledhu" |
| E0502 | role mismatch or too many phrases for a role | |
| E0503 | formal bound twice | |
| E0504 | unknown label | |
| E0505 | keyword-only formal passed positionally | |
| E0506 | invalid spread | |
| E0507 | pipe slot already bound | |
| E0508 | hole role mismatch | |
| **Ownership** | | |
| E0601 | use after move | "`a` ni already `b` ki icchesav kada! Malli vaadalevu." |
| E0602 | write to a non-mutable place | "`x` maare kaadhu, maarchalevu." |
| E0603 | move out of a borrow or index | |
| E0604 | conflict with a shared loan | |
| E0605 | conflict with a mutable loan | |
| E0606 | owner dropped or moved while borrowed | |
| E0607 | `&maare` of a non-mutable place | |
| E0608 | returned reference with no single source | |
| E0609 | use of a maybe-uninitialized value | |
| E0610 | `&maare` argument needs an explicit borrow | "`&maare x` ani raayi" |
| **Patterns** | | |
| E0701 | non-exhaustive match | "`X` case marchipoyav!" |
| E0703 | refutable pattern in a binding | |
| E0704 | constructor arity | |
| E0705 | float literal pattern | |
| E0706 | OR-pattern alternatives bind different names or types | |
| E0707 | invalid range pattern | |
| W0004 | unreachable arm | |
| **Traits** | | |
| E0801 | trait not implemented | |
| E0802 | missing trait method | |
| E0803 | orphan implementation | |
| E0804 | overlapping implementations | |
| E0805 | infinite instantiation | |
| E0806 | invalid `CopyKottochu` | |
| E0807 | not derivable | |
| **Modules** | | |
| E0901 | module not found | |
| E0902 | constant cycle | |
| E0903 | no `modhalu` | |
| E0904 | multiple `modhalu` | |
| E0905 | invalid `ante.toml` or package layout | |
| **FFI** | | |
| E1001 | unsafe operation outside `naa_risk` | |
| E1002 | non-FFI-safe type | |
| E1003 | invalid export | |
| **Constant evaluation** | | |
| E1101 | not a constant | |
| E1102 | trap during constant evaluation | |
| E1103 | step limit exceeded | |
| **Edition** | | |
| E1201 | feature not in this edition | |
| **Warnings** | | |
| W0001 | unused binding | |
| W0002 | unnecessary `maare` | |
| W0003 | unreachable code | |
| W0005 | unused import | "`x` import chesaru kaani ekkada vaadaledhu" |
| W0100 | deprecated word (§23.5) | "ee padham paathadhi, `kottha_padham` vaadandi" |

Warnings are reported only when a program has no errors, and never stop a build. A binding whose name starts with `_` is exempt from W0001.

---

# Appendix C — Conformance suite

## C.1 Test file format

Tests live in `tests/<category>/<id>_<name>.ante`. Expectations are written in leading comments:

```ante
# category: run-pass | compile-fail | runtime-trap | build-pass | integration
# expect-stdout: 30
# expect-error: E0601 @ 4:5
# expect-warning: W0001 @ 5:5
# expect-trap: T01 @ 3:9
# expect-exit: 101
# level: core | torch | python
```

A test may have several `expect-stdout` lines, which are matched in order.

A **package test** is a folder holding `ante.toml` and `src/`. Its expectations are the leading `#` comments of `ante.toml`, and error spans name the file: `# expect-error: E0303 @ src/modhalu.ante:1:14`. A compile-fail test passes when the expected code is reported with the expected primary span. Other errors may also be reported.

## C.2 Seed cases

| ID | Case | Expected |
|---|---|---|
| L001 | Tab in indentation | E0102 |
| L002 | Dedent to an unknown level | E0103 |
| L003 | Line joining by rules (a)–(d) of §4.2.3 | parses; build-pass |
| L004 | `1__0`, `0x_F` | E0106 |
| L005 | `"\u{D800}"` | E0108 |
| S001 | `10 ni, 20 tho kalupu()` | stdout `30` |
| S002 | `x ni f() g()` | E0201 |
| S003 | `10 ni 20 tho kalupu()` (missing comma) | E0203 |
| S004 | `kalupu(1, 2)` | E0204 |
| S005 | `(a, b)` is a tuple; `(a, b f())` is a call | run-pass |
| S006 | `0 nunchi 5 lopu okko i ki:` prints 0 to 4 | run-pass |
| S007 | `"f.txt" nunchi chadhuvu()` with a `nunchi` formal | run-pass |
| S008 | `x ni f()?` propagates the call result | run-pass |
| S009 | `handlers[0]()` on a vector | E0401 (generic-call parse), hint `(handlers[0])()` |
| S010 | `x += 1` inside an expression | E0210 |
| S011 | `@pariksha` followed by a statement | E0208 |
| S012 | Statement at module level | E0209 |
| S013 | Phrase with no verb (`10 ni`) | E0202 |
| S014 | `a < b < c` | E0417 |
| S015 | One file exercising D4, D5, D7, D10, D11, pipe holes, `lo okko` and `batti :` | parses; build-pass |
| B001 | `20 tho, 10 ni kalupu()` | `30` |
| B002 | `3 ni c ga, 1 ni a ga, 2 ni b ga kalupu_moodu()` | a=1, b=2, c=3 |
| B003 | `1 ni, 3 ni c ga, 2 ni kalupu_moodu()` | a=1, b=2, c=3; source-order effects |
| B004 | Same formal labelled twice | E0503 |
| B005 | Unknown label without a collector | E0504 |
| B006 | `tho` phrase for an `ni`-only function | E0502 |
| B007 | Missing `tho` argument | E0501 |
| B008 | `peru_thone` formal passed positionally | E0505 |
| B009 | `...xs` (`varusa`) into fixed formals | E0506 |
| B010 | `...xs` into `migathavi` | correct sum |
| B011 | Default parameter used | value printed |
| P001 | `x \|> 5 tho kalupu()` | pipe binds the first formal |
| P002 | `x \|> _ ni a ga, 5 tho kalupu()` | hole binds `a` |
| P003 | Two holes | E0213 |
| P004 | Pipe into an already-bound first slot | E0507 |
| P005 | `owned \|> choopinchu()` then reuse `owned` | automatic borrow; run-pass |
| P006 | Pipe hole whose role no formal has | E0508 |
| T001 | `ankam8` 127 + 1 | T01, exit 101 |
| T002 | Integer division by zero | T02 |
| T003 | `xs[5]` on length 3 | T03 |
| T004 | `1 << 64` with a runtime count | T04 |
| T005 | `xs[3 nunchi 1 lopu]` | T10 |
| T006 | `ledhu.kachithanga()` | T06 |
| T007 | `(1 == 2) nijame()` | T07 |
| T008 | `0.0 / 0.0 == 0.0 / 0.0` | prints `kaadhu` |
| T009 | `-128 : ankam8` literal | run-pass |
| T010 | `ankam32` + `aasajeevi ankam32` | E0408 |
| T011 | `x ni maarchu[ankam8]()` with `x : ankam32` | E0409 |
| T012 | `300 ni kudhirithe_maarchu[ankam8]()` | `ledhu` |
| T013 | `300 ni kolpothu_maarchu[ankam8]()` | `44` |
| T014 | `(-1.5) ni kolpothu_maarchu[ankam32]()` | `-1` |
| T015 | `127 ni, 1 tho lekkalu.trupthiga_kalupu()` on `ankam8` | `127` |
| T016 | `127 ni, 1 tho lekkalu.chuttu_kalupu()` on `ankam8` | `-128` |
| T017 | Strict float sum: same bits on every run | run-pass |
| T018 | `@vegam` sum is within the documented tolerance and vectorized (inspect IR) | run-pass |
| T019 | `a < b < c` | E0417 |
| T020 | Non-boolean `aithe` condition | E0403 |
| T021 | `padham[0]` | E0413 |
| T022 | Annotation and value disagree | E0401 |
| T023 | `300 : ankam8` | E0402 |
| T024 | `xs ante []` with no element type | E0404 |
| T025 | Unknown record field | E0405 |
| T026 | Unknown method | E0406 |
| T027 | `+` on a record without `Kalupochu` | E0407 |
| T028 | A path that ends without `icchey` | E0410 |
| T029 | `?` in a `khali` function | E0411 |
| T030 | `?` with an unconvertible error type | E0412 |
| T031 | Index by a `padham` | E0414 |
| T032 | Field missing from a record literal | E0416 |
| T033 | Record containing itself | E0418 |
| T034 | Duplicate constant map key | E0419 |
| T035 | Function call in a constant | E1101 |
| T036 | Literals, operators, records, enums, closures, adapters, conversions | build-pass |
| O001 | Use after move | E0601 |
| O002 | Assign to a non-`maare` binding | E0602 |
| O003 | Move out of `xs[0]` (non-Copy element) | E0603 |
| O004 | Write while a shared loan is live | E0604 |
| O005 | Two `&maare` live at once | E0605 |
| O006 | Return a reference to a local | E0606 |
| O007 | Reference stored in a `rakam` field | E0415 |
| O008 | Two reference parameters, reference returned | E0608 |
| O009 | Owned value to a `&maare` formal | E0610 |
| O010 | `v.enni() ni v.cherchu()` | run-pass (two-phase receiver) |
| O011 | Shared loan ends at its last use (NLL) | run-pass |
| O012 | Drop order: reverse initialization; `sardhu` before fields | expected stdout |
| O013 | `vellagane` runs LIFO, including on `?` | expected stdout |
| O014 | Temporary lifetime extension `r ante &f()` | run-pass |
| O015 | `&maare` of a non-`maare` binding | E0607 |
| O016 | `&maare idhi` method on a non-`maare` binding | E0607 |
| M001 | Non-exhaustive `sare_thappu` match | E0701 |
| M002 | Refutable pattern in a binding | E0703 |
| M003 | Copy binding mode through `&` | run-pass |
| M004 | Variant pattern with the wrong number of fields | E0704 |
| M005 | Nested exhaustive match over tuples and `undochu` | build-pass |
| W004 | Arm covered by earlier arms | W0004 |
| G001 | Generic `modhatidhi[T]` on `ankam` and `padham` | run-pass |
| G002 | Missing bound used in a generic body | E0801 |
| G003 | Orphan implementation | E0803 |
| G004 | `@automatic Polchochu, Choopinchochu` on a record | run-pass |
| G005 | `CopyKottochu` together with `Sardhukochu` | E0806 |
| G006 | Method pro-drop `balance` → `naa.balance` | run-pass |
| G007 | Closure with a borrowed capture, returned | E0415 |
| G008 | `theesuko` closure returned | run-pass |
| G009 | Implementation missing a required method | E0802 |
| G010 | Same trait implemented twice for a type | E0804 |
| G011 | `@automatic` with a field lacking the trait | E0807 |
| G012 | User trait with a default method, called generically | run-pass |
| N001 | Undefined name | E0301 |
| N002 | Name used before its binding | E0306 |
| N003 | Redefinition in the same scope | E0308 |
| N004 | Duplicate item | E0302 |
| N005 | Two `modhalu` | E0904 |
| N006 | Unknown module | E0901 |
| N007 | Unknown imported item | E0304 |
| N008 | Constant cycle | E0902 |
| N009 | Private type in a public signature | E0303 |
| N010 | Unknown enum variant | E0301 |
| N011 | OR-pattern alternatives bind different names | E0706 |
| N012 | Imports, nested scopes, shadowing, `naa.` pro-drop | build-pass |
| PK001 | Package: import a public function and type, by name and through the module | build-pass |
| PK002 | Package: import a private item | E0303 |
| PK003 | Package: `bin` without `src/modhalu.ante` | E0903 |
| PK004 | Package: entry module without `modhalu()` | E0903 |
| PK005 | Package: unknown local module | E0901 |
| PK006 | Package: nested modules importing each other in a cycle | build-pass |
| PK007 | Package: `lib` with `src/lib.ante` | build-pass |
| W001 | Binding never read | W0001 |
| W002 | `maare` binding never changed | W0002 |
| W003 | Statement after `icchey` | W0003 |
| W005 | Import never used | W0005 |
| D001 | `jodi` iteration keeps insertion order | run-pass |
| D002 | `jodi` replacement keeps the key's position | run-pass |
| D003 | f-string interpolation | stdout matches |
| X001 | Program without `python`/`torch` imports | no CPython or LibTorch linkage |
| X002 | `modhalu` returns `thappu(e)` | exit 1, stderr `thappu: …` |
| X003 | `bayata` call outside `naa_risk` | E1001 |
| X004 | `@bayata "ante_kalupu"` export callable from C | integration |
| X005 | Async syntax | E1201 |
| TT001 | Appendix D.6 training step, built without `python` | links LibTorch only (torch level) |
| TT002 | `x[_, -1]` on a 2-D tensor | last column (torch level) |
| TT003 | Shape-mismatched matmul | T05; `torch.choosi` variant returns `thappu` (torch level) |
| PY001 | Python exception | `thappu(PythonThappu)` (python level) |
| PY002 | `ga` label on a positional-only parameter | E0505 (python level) |

**Release gate.** Before `antec` 1.0, the suite must have at least 10 cases per production and at least 10 per diagnostic family, plus parser fuzzing and differential tests between the AArch64 and x86-64 backends.

---

# Appendix D — Example programs

### D.1 Hello ("Ante" Sundharaniki)

```ante
sundharaniki() -> khali ante:
    "Telugu lo code cheyyadam vachindhanta! 🎉" ni choopinchu()

vachindhanta(evariki: padham, emi: padham ni) -> khali ante:
    f"{evariki} {emi} vachindhanta! 🎉" ni choopinchu()

modhalu() -> khali ante:
    sundharaniki()
    "Meeku", "\"Ante\" lo code cheyyadam" ni vachindhanta()
```

The declaration line reads as a sentence: "sundharaniki ante: Telugu lo code cheyyadam vachindhanta!" The second call binds the unmarked phrase to `evariki` and the `ni` phrase to `emi` (§11.2). This is also `examples/ante_sundharaniki.ante` in the repository.

### D.2 Roles, defaults and labels

```ante
@automatic Choopinchochu
rakam Manishi ante:
    peru: padham
    vayasu: ankam32
    ooru: padham

manishi_chesko(peru: padham ni, vayasu: ankam32 ni = 18,
               ooru: padham ni = "Hyderabad") -> Manishi ante:
    Manishi{peru: peru, vayasu: vayasu, ooru: ooru} icchey

modhalu() -> khali ante:
    m ante "Ravi" ni, "Vizag" ni ooru ga manishi_chesko()
    m ni choopinchu()       # Manishi{peru: Ravi, vayasu: 18, ooru: Vizag}
    f"{m.peru} ki {m.vayasu} ellu" ni choopinchu()
```

### D.3 Methods, `naa` and pro-drop

```ante
rakam Account ante:
    balance: ankam64

Account panulu ante:
    kotthadhi() -> Account ante:
        Account{balance: 0} icchey

    jama(naa: &maare idhi, amount: ankam64 ni) -> khali ante:
        naa.balance += amount

    choopu(naa: &idhi) -> khali ante:
        balance ni choopinchu()

modhalu() -> khali ante:
    maare acc ante Account.kotthadhi()
    500 ni acc.jama()
    acc.choopu()            # 500
```

### D.4 Enums, `pette` and matching

```ante
edho_okati Chettu ante:
    Aaku
    Komma(pette[Chettu], ankam, pette[Chettu])

motham_lekka(c: &Chettu ni) -> ankam ante:
    c batti:
        Chettu.Aaku aithe:
            0 icchey
        Chettu.Komma(edama, v, kudi) aithe:
            (edama ni motham_lekka()) + v + (kudi ni motham_lekka()) icchey

modhalu() -> khali ante:
    aaku ante Chettu.Aaku ni pette.kotthadhi()
    chettu ante Chettu.Komma(aaku, 5, (Chettu.Aaku ni pette.kotthadhi()))
    chettu ni motham_lekka() |> choopinchu()      # 5
```

`edama` (left) and `kudi` (right) bind as `&pette[Chettu]`, which coerces to `&Chettu` (§9.4.2c). `v` binds by copy (§14.2).

### D.5 Errors, files and adapters

```ante
file nunchi chadhuvu, FileThappu thechuko

modhalu() -> sare_thappu[khali, FileThappu] ante:
    text ante "marks.txt" ni chadhuvu()?
    maare marks: varusa[ankam] ante []
    "\n" ni text.vidagottu() lo okko line ki:
        line.ankam_ga() batti:
            sare(n) aithe:
                n ni marks.cherchu()
            thappu(_) aithe:
                vadhiley
    marks.sort_cheyyi()
    marks |> 3 ni modhati() |> varusa_ga() |> choopinchu()
```

### D.6 Strict and fast dot product

```ante
edho_okati PodavuThappu ante:
    Saripoledhu

dot(a: &mukka[amsam32] ni, b: &mukka[amsam32] tho)
    -> sare_thappu[amsam32, PodavuThappu] ante:
    a.enni() != b.enni() aithe:
        thappu(PodavuThappu.Saripoledhu) icchey
    maare total: amsam32 ante 0.0
    a, b kalipi() lo okko (x, y) ki:
        total += x * y
    sare(total) icchey

@vegam
dot_vegam(a: &mukka[amsam32] ni, b: &mukka[amsam32] tho)
    -> sare_thappu[amsam32, PodavuThappu] ante:
    a.enni() != b.enni() aithe:
        thappu(PodavuThappu.Saripoledhu) icchey
    maare total: amsam32 ante 0.0
    a, b kalipi() lo okko (x, y) ki:
        total += x * y
    sare(total) icchey
```

The strict version gives bit-exact results and can't be vectorized. The `@vegam` version may reorder the sum so it vectorizes. In production code, use `a ni, b tho lekkalu.dot()`.

### D.7 PyTorch training step (Annex T)

```ante
torch thechuko

rakam Model ante:
    l1: torch.nn.Linear
    l2: torch.nn.Linear

Model panulu ante:
    kotthadhi() -> Model ante:
        Model{
            l1: 784 ni, 128 ni torch.nn.Linear(),
            l2: 128 ni, 10 ni torch.nn.Linear(),
        } icchey

    forward(naa: &idhi, x: &torch.Tensor ni) -> torch.Tensor ante:
        x ni l1.forward() |> torch.relu() |> l2.forward() icchey

    parameters(naa: &idhi) -> varusa[torch.Tensor] ante:
        [l1.weight.xerox(), l1.bias.xerox(), l2.weight.xerox(), l2.bias.xerox()] icchey

train_step(model: &maare Model ni, opt: &maare torch.optim.SGD tho,
           x: &torch.Tensor kosam, y: &torch.Tensor ki) -> amsam32 ante:
    pred ante x ni model.forward()
    loss ante pred ni, y ni torch.nn.functional.cross_entropy()
    opt.zero_grad()
    loss.backward()
    opt.step()
    loss.item() icchey

modhalu() -> khali ante:
    maare model ante Model.kotthadhi()
    maare opt ante model.parameters() ni, 0.01 ni lr ga torch.optim.SGD()
    x ante 64 ni, 784 ni torch.randn()
    y ante 0 ni, 10 ni, (64,) ni torch.randint()
    0 nunchi 100 lopu okko step ki:
        loss ante &maare model ni, &maare opt tho, x kosam, y ki train_step()
        step % 10 == 0 aithe:
            loss ni choopinchu()
```

`train_step` uses four distinct roles, so its call reads unambiguously in any order.

---

# Appendix E — Implementation roadmap for `antec`

The recommended architecture: Rust, a hand-written lexer and recursive-descent parser (the D-rules need bounded lookahead and local backtracking, which suits hand-written code better than a generator), an arena AST, then HIR, then typed HIR, then MIR (a CFG of basic blocks with explicit places, moves, loans and drops), then LLVM via the `inkwell` crate. Keep a **MIR interpreter** alongside it, so the semantics can be tested before code generation exists.

| Milestone | Deliverable | Conformance gate |
|---|---|---|
| M1 | Lexer and layout (§4) | L-series; golden token streams |
| M2 | Parser and AST, `antec emit ast`, `antec fmt` round-trip | S-series syntax cases; fuzzing finds no panics |
| M3 | Modules, name resolution, visibility (§7–8) | E03xx and E09xx |
| M4 | Type checker for monomorphic code: literals, operators, records, enums, patterns (§9, §10, §14) | E04xx, E07xx |
| M5 | Call binding, automatic borrows, pipes (§11–12) | B- and P-series |
| M6 | MIR lowering and MIR interpreter, with traps and drop/`vellagane` order (Part V) | first run-pass and runtime-trap cases |
| M7 | LLVM code generation for core types; runtime in "Ante" plus a small C shim for allocation, I/O and traps | the same cases, compiled natively on AArch64 and x86-64 |
| M8 | Ownership and borrow checker on MIR (§15) | O-series |
| M9 | Generics, traits, monomorphization, `@automatic`, closures (§13, §10.7) | G-series |
| M10 | Standard library (§21), iteration adapters, f-strings | D-series; Appendix D programs |
| M11 | FFI, `naa_risk`, `@C`, exports (§22) | X-series |
| M12 | `@vegam`, `lekkalu.dot` vectorization, performance benchmarks | T017/T018 |
| M13 | Annex T (LibTorch shim generated from `Declarations.yaml`) | TT-series |
| M14 | Annex P (CPython embedding, `.pyi` stubs) | PY-series |

Core 1.0 is complete at M11. M12–M14 are independent and can proceed in parallel.

---

# Appendix F — Deferred features

These are reserved and rejected with E1201 where syntax exists:

- concurrency (`thread`, `Pampochu`, `Panchukovochu`, `Thaalam` locks, channels, atomics);
- async/await, generators and `yield`;
- trait objects, associated types and specialization;
- lifetime annotations, and references stored in structs;
- macros and reflection;
- Unicode identifiers;
- open-ended slices and spread-holes in pipes;
- function overloading;
- else-less expression forms of `aithe`/`batti`;
- `kitiki` strided views and `panchukunna` reference counting;
- a native tensor type and MLIR codegen;
- the locative receiver form (`account lo 500 ni jama()`);
- constant function calls.

---

# Appendix G — Change history

Edition 1.0 starts at draft r1, which is the first public revision. Every later change to this spec that affects programs is listed here, newest first, with its revision number and the release that shipped it (see `docs/VERSIONING.md`).

| Revision | Release | Change |
|---|---|---|
| r2 | `antec` 0.2.0 | Clarifications from the type checker (M4): automatic dereference of scalar references in operators (§10.5.1); method lookup skips reference candidates (§13.4.2); numbers implement the arithmetic traits (§13.5); `jalleda_pattu`'s predicate receives `&I` (§21.4.2); Appendix D.7 defines `Model.parameters`; new seed cases T022–T036, P006, O015–O016, M004–M005, W004, G009–G012. |
| r1 | `antec` 0.1.0 | First public draft of Edition 1.0. |

---

# Appendix H — References

- Python language reference: https://docs.python.org/3/reference/
- The Rust Reference (ownership, patterns, traits): https://doc.rust-lang.org/reference/
- Rust RFC 2094, non-lexical lifetimes: https://rust-lang.github.io/rfcs/2094-nll.html
- LLVM Language Reference, fast-math flags: https://llvm.org/docs/LangRef.html#fast-math-flags
- `inkwell` (LLVM bindings for Rust): https://github.com/TheDan64/inkwell
- `tch-rs` (Rust LibTorch bindings): https://github.com/LaurentMazare/tch-rs
- PyTorch C++ API: https://pytorch.org/cppdocs/
- DLPack: https://dmlc.github.io/dlpack/latest/
- IEEE 754-2019, Standard for Floating-Point Arithmetic

---

# Appendix I — Self-hosting readiness

**Goal.** A later `antec` written in "Ante" compiles itself. The Rust `antec` (stage 0) is the first compiler, and this appendix makes sure "Ante" 1.0 can express a compiler, so that self-hosting needs no breaking language changes.

## I.1 What a compiler written in "Ante" needs

| Need | Status in 1.0 | Where |
|---|---|---|
| Read source files and list directories | **added** | `file.chadhuvu`, `file.list_cheyyi`, `file.folderaa`, path helpers (§21.3) |
| Byte-level scanning of source | present, extended | `padham.baitlu()`, `byte_bhaagam`, `baitlu_nunchi` (§21.2) |
| Character classes for the lexer | **added** | `digit_aa`, `letter_aa`, `space_aa`, `alnum_aa`, `ascii_aa` |
| Exact number parsing in any base | **added** | `ankam_ga(base)`, `aasajeevi_ga(base)`, correctly rounded `amsam_ga` |
| Exact float output for IR | **added** | `bits()`, `bits_nunchi()`, `hex_ga()` |
| Fast output building | **added** | `Raatha` string builder |
| Syntax trees | present | `edho_okati` + `pette`, exhaustive `batti` |
| Matching many token kinds at once | **added** | OR-patterns, range patterns (§5.5) |
| Symbol tables and interning | present, extended | `jodi`, `@automatic HashCheyochu`, `maare_thechu`, `lekapothe_cherchu` |
| One error type across passes | **added** | `NunchiRavochu` conversion for `?` (§10.6) |
| Running the linker and tools | **added** | `system.nadupu` |
| Target detection | **added** | `system.os()`, `system.arch()`, `[build] target` |
| Calling the LLVM C API | present, clarified | `bayata "C"`, `*khali` opaque handles (§22) |
| Writing the runtime and collections in "Ante" | **added** | `memory` module inside `naa_risk` |
| Deep recursion on large inputs | **added** | `[build] stack`, minimum 8 MB |
| Reproducible output (stage 2 = stage 3) | **added** | §23.2.2 determinism; stable sorts; insertion-ordered `jodi` |
| Locating the standard library | **added** | §23.2.1 installation layout |
| Cyclic module imports | clarified | §7.6 |

## I.2 Design pattern for large programs: indices instead of references

"Ante" 1.0 forbids references inside records and collections (§9.3). A compiler keeps its data in arenas, and items refer to each other through small `CopyKottochu` IDs. rustc uses the same pattern.

```ante
@automatic CopyKottochu, Polchochu, HashCheyochu, Choopinchochu
rakam NodeId ante:
    i: ankam32

rakam Ast ante:
    nodes: varusa[Node]

Ast panulu ante:
    cherchu_node(naa: &maare idhi, n: Node ni) -> NodeId ante:
        id ante NodeId{i: naa.nodes.enni() ni kolpothu_maarchu[ankam32]()}
        n ni naa.nodes.cherchu()
        id icchey

    node(naa: &idhi, id: NodeId ni) -> &Node ante:
        &naa.nodes[id.i] icchey
```

`node` returns a reference under rule §9.3.2(c): `naa` is its only reference parameter.

## I.3 Known gaps (fine for 1.0, planned later)

| Gap | Workaround in 1.0 | Planned |
|---|---|---|
| No trait objects | an `edho_okati` of the variants, plus `batti` | trait objects (Appendix F) |
| No threads | a single-threaded compiler | the concurrency edition |
| No match guards | a nested `aithe` inside the arm | guards (`pattern, cond aithe:`) |
| No labelled `aapey` | a flag variable | labelled loops |
| No references in records | IDs (I.2) | lifetime parameters, if ever needed |
| `NunchiRavochu.nunchi` can't be called by name: `nunchi` is a role word | `?` converts errors automatically; write a named conversion function for explicit calls | a new method name, by vocabulary RFC |

None of these block self-hosting.

## I.4 Bootstrap procedure

1. **Stage 0:** the Rust `antec`. It is maintained until stage 1 passes the full Core suite, then frozen at that version.
2. **Stage 1:** the "Ante" sources of `antec`, compiled by stage 0.
3. **Stage 2:** the same sources compiled by stage 1. **Stage 3:** compiled by stage 2. Stage 2 and stage 3 binaries must be byte-identical (§23.2.2), and both must pass the conformance suite.
4. **Long-term bootstrap:** the self-hosted compiler gains a C backend, and each release commits a generated C version of `antec`, so any system with a C compiler can build stage 1 without Rust.

The self-hosted compiler should first emit textual LLVM IR and run `clang` through `system.nadupu`. Linking against the LLVM C API is a later optimization.
