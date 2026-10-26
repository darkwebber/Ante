# The "Ante" Programming Language — Specification, Edition 1.0

**Status:** Final draft for implementation. Normative for the reference compiler `antec`.  
**Revision:** Edition 1.0, draft r10. Vocabulary may still change before `antec` 1.0.0, under the rules in `docs/VERSIONING.md` (§23.5).  
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
- Part IX — Concurrency and services: §24 Tasks and sharing, §25 Bytes and data formats, §26 Time, §27 Randomness and cryptography, §28 Networking and HTTP, §29 Storage, logs and configuration
- Annex T — PyTorch binding (optional conformance level)
- Annex P — Python bridge (optional conformance level)
- Annex W — Web applications (optional conformance level)
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
| **"Ante" Services 1.0** | Core plus Part IX (tasks, data formats, time, cryptography, networking, storage) |
| **"Ante" Web 1.0** | Services plus Annex W (web applications) |
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
5. **Predictable.** Left-to-right evaluation, no implicit conversions, the same arithmetic in debug and release. The optimizer may change speed but not behavior, except inside an explicit `@twaraga` opt-in.
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
| Types | `ankam` `ankam8` `ankam16` `ankam32` `ankam64` `aasajeevi` `amsam` `amsam32` `amsam64` `ato_ito` `padham` `aksharam` `jatha` `jodi` `undochu` `varusa` `mukka` `sare_thappu` `pani` `pette` `khali` `raadhu` `idhi` `panchukunna` (§24.4) `edhaina` (Annex P) |
| Reserved for later editions | `kitiki` `gurinchi` `async` `await` `yield` |

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
- `sql"…"` (Services, §29.3.1) is lexed exactly like an f-string, with the prefix `sql` instead of `f`. Its interpolations become bound query parameters, not text.
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
annotation_arg ::= STRING | module_path (',' module_path)*

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
field         ::= annotation* 'andhariki'? NAME ':' type NEWLINE

enum_decl     ::= 'edho_okati' NAME generic_params? 'ante' ':' NEWLINE INDENT variant+ DEDENT
variant       ::= annotation* NAME ('(' type (',' type)* ','? ')')? NEWLINE

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

A bare `NAME` pattern is always a new binding. A variant is always written as a qualified path with at least one `.` (`Color.Erupu`), except the prelude constructors `undhi`, `ledhu`, `sare` and `thappu`. Float literal patterns are E0705.

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

8.4 **Namespaces.** Each module has one value namespace (functions, constants, local bindings) and one type namespace (records, enums, traits, imported modules). Defining the same name twice in one namespace is E0302. Variants live inside their enum (`Color.Erupu`). Methods live inside their type.

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
| `pani[(P…) -> R]` | Callable value (§10.7) | no; using a `pani` place shares it instead of moving it (§10.7.4) |
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

10.5.2 For other types, operators resolve to trait methods (§13.5): `+` → `Kalupochu.kalupu`, `-` → `Theeseyochu.theesey`, `*` → `Guninchochu.guninchu`, `/` → `Panchochu.panchu`, `%` → `Migulochu.migulu`, unary `-` → `Thippochu.thippu`, `@` → `MatmulCheyochu.matmul`, `==`/`!=` → `Polchochu.okateyna`, `< <= > >=` → `VarusaPettochu.polchu`. Operands are passed by automatic shared borrow. An operator with no implementation is E0407. `padham + padham` is provided and allocates.

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
| `e?` | `e : sare_thappu[T, E]` gives `T`. The enclosing function must return `sare_thappu[_, F]` where `F = E` or `F` implements `NunchiRavochu[E]`, in which case the error is converted with `F.daani_nunchi` (E0411, E0412) |
| `f"…{c}…"` | each `c : C` with `C: Choopinchochu`; result `padham` |
| `undhi(e)`, `sare(e)`, `thappu(e)`, `E.V(e…)` | the constructor's type, with arity checked (E0704) |
| `T{f: e, …}` | every field exactly once (E0416); fields must be visible |
| `a nunchi b lopu/varaku` | `a, b : I` for an integer type `I`; result `Paridhi[I]` (§21.4) |

### 10.7 Closures and `pani`

10.7.1 A closure `(x: T ni, …) => body` has type `pani[(T ni, …) -> R]`, where `R` is the type of `body`. Parameter types and roles may be left out when an expected `pani` type supplies them.

10.7.2 **Captures.**
- A closure captures by shared borrow any outer local it uses (`naa` included).
- With the `theesuko` ("take it") prefix, it captures every outer local it mentions by **move** (a copy for `CopyKottochu` types). The closure owns those values, but it may run many times and its copies share them, so its body can't move a captured value out (E0603). Use `.xerox()` instead.
- A closure body can't change anything it captured: assigning to it, borrowing it with `&maare` or calling a `&maare idhi` method on it, directly or through a field, index or `*`, is E0611. Pass the place to the closure as a `&maare` parameter instead.
- A closure can't capture a raw pointer, or a value containing one, and a `theesuko` closure can't capture a `&maare` reference (E0612): its copies would share them.

A closure with any borrow captures is RC (§9.3). The borrow checker knows which closure values hold borrows (§15.4.6).

10.7.3 A named function used as a value has a `pani` type with its declared parameter roles. Calling a local of `pani` type uses the call syntax of §11 with the roles from its type.

10.7.4 **Copies.** `pani` doesn't implement `CopyKottochu`, but a `pani` place used as a value isn't moved either: it makes another handle to the same closure, in O(1). All handles share the captured values, which are dropped, last capture first, when the last handle is dropped. `pani` implements `XeroxTheeyochu` (the same O(1) copy, explicitly). Because §10.7.2 rules out the captures that couldn't be shared, every `pani` is `Pampochu` and `Panchukovochu` (§24.3).

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

A phrase whose operand is a place of type `&maare T` is **reborrowed** rather than moved: the argument is `&maare *operand`, a new loan that ends when the call's result no longer needs it (§15.4.2). So `r ni penchu()` can be written twice with the same `r`. A binding (`s ante r`) still moves `r`.

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
| `Polchochu` | `okateyna(naa: &idhi, inkokati: &idhi ni) -> ato_ito` | equality, `==` ("are they the same?") |
| `VarusaPettochu` | `polchu(naa: &idhi, inkokati: &idhi ni) -> Kramam` | total order, `<` etc. ("can be put in a line") |
| `HashCheyochu` | `hash_cheyyi(naa: &idhi, h: &maare Hasher ni) -> khali` | hashing; must agree with `Polchochu` |
| `Choopinchochu` | `padham_ga(naa: &idhi) -> padham` | display ("can be shown") |
| `Okkokkati[I]` | `tharvatha(naa: &maare idhi) -> undochu[I]` | iteration, "one by one" (`tharvatha` = "next") |
| `Kalupochu`, `Theeseyochu`, `Guninchochu`, `Panchochu`, `Migulochu` | `kalupu`/`theesey`/`guninchu`/`panchu`/`migulu(naa: &idhi, inkokati: &idhi ni) -> idhi` | `+ - * / %` |
| `Thippochu` | `thippu(naa: &idhi) -> idhi` | unary `-` ("flip") |
| `MatmulCheyochu` | `matmul(naa: &idhi, inkokati: &idhi ni) -> idhi` | `@` |
| `NunchiRavochu[F]` | `daani_nunchi(vishayam: F ni) -> idhi` | conversion "can come from F" (`daani_nunchi`: "from that one"); used by `?` (§10.6) |

`Hasher` is a prelude record with `x ni h.kalupu()` for every integer type, `aksharam`, `ato_ito` and `&mukka[aasajeevi ankam8]`, and `h.chaalu() -> aasajeevi ankam64` ("finish"). The algorithm is implementation-defined but must be deterministic for a fixed seed.

`Kramam` is `edho_okati Kramam ante: Thakkuva, Samaanam, Ekkuva` (less, equal, more).

**Built-in implementations:**
- Numeric types, `ato_ito`, `aksharam` and `khali` implement `CopyKottochu`, `XeroxTheeyochu`, `Polchochu` and `Choopinchochu`.
- Integers, `ato_ito`, `aksharam` and `khali` also implement `VarusaPettochu` and `HashCheyochu`.
- Integers and floats implement `Kalupochu`, `Theeseyochu`, `Guninchochu`, `Panchochu` and `Migulochu`; signed integers and floats also implement `Thippochu`, and `padham` implements `Kalupochu`. Generic code bounded by these traits can therefore use the operators on numbers.
- **Floats implement neither `VarusaPettochu` nor `HashCheyochu`.** Their `<` and `==` are built-in IEEE operations (§18.3).
- `padham` implements everything except `CopyKottochu`.
- `jatha`, `[T;N]`, `varusa`, `undochu`, `sare_thappu`, `jodi` and `pette` implement each trait whenever their components do. The exceptions: `jodi` has no `VarusaPettochu`/`HashCheyochu`, and only the types marked in §9.1 have `CopyKottochu`.

**Built-in display.** The built-in `padham_ga` shows integers in decimal; floats in the shortest form that reads back exactly, with `.0` on whole numbers (`1.0`, `0.30000000000000004`), and `NaN`, `inf`, `-inf`; `ato_ito` as `avunu`/`kaadhu`; text and characters as themselves, also inside collections; tuples as `(a, b)` (a one-element tuple as `(a,)`); arrays, `varusa` and `mukka` as `[a, b]`; `jodi` as `{k: v, …}` and `gumpu` as `{a, …}` in insertion order; `undochu` as `undhi(x)`/`ledhu`; `sare_thappu` as `sare(x)`/`thappu(e)`; `khali` as `()`; `pette` and references as their contents.

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

Moving out of an index place, through a reference, or out of a `theesuko` closure's captured value (§10.7.2) is E0603. So is moving a field out of a value whose type implements `Sardhukochu`, because `sardhu` needs the whole value (§19.3). Use `.xerox()`, `undochu` methods, or `varusa.theesey()` instead.

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
- Outside `@twaraga`, implementations must not reassociate, contract into FMA, or assume the absence of NaN/Inf. Results are bit-reproducible for a given target, except for transcendental functions in `lekkalu`, whose accuracy is documented per implementation.
- **`@twaraga` ("speed")** on a function allows, for `amsam` operations written **lexically inside that function**, reassociation (including reordering reductions) and FMA contraction. It doesn't allow assuming NaN or Inf are absent, doesn't affect integer semantics, and doesn't propagate into called functions. In LLVM terms: `reassoc` and `contract` only.

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

19.3 **Drop.** When a scope ends, by falling through, `icchey`, `aapey`, `vadhiley`, or `?` propagation, its initialized locals are dropped in reverse order of initialization. A maybe-moved local carries a runtime drop flag, and so does each field path moved out of it: dropping a partially moved value drops only the fields still initialized, in the order below. Assigning to a moved place drops nothing; it just re-initializes the place.

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

20.3 **Traps** end the process (in Services, a trap outside the first task ends only its task, §24.5). There is no unwinding and no catching, and cleanup isn't guaranteed. The message format is:

```text
kompa munigindhi! <reason> (<file>:<line>:<col>)
```

For T08 and T09 the position may be left out (`kompa munigindhi! <reason>`) when the implementation can't know it, as when compiled code detects a stack overflow through a guard page.

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
| T11 | opening a poisoned `Thaalam` (§24.4) |
| T12 | a task opening a `Thaalam` it already holds (§24.4) |
| T13 | a Python exception where no `sare_thappu` can carry it: attributes, indexing, operators, iteration (Annex P.12.3) |
| T14 | the Python environment couldn't be loaded, or an import failed, at program start (Annex P.18.5) |

20.4 Output to stdout is buffered and flushed at normal exit, before a trap message, and by `system.flush()`.

20.5 **Determinism.** Apart from I/O, time, environment, `HashCheyochu` seeds (which may be randomized per process, though `jodi` iteration stays insertion-ordered), and documented `@twaraga` effects, program behavior is deterministic.

20.6 **Implementation-defined** behavior, which each implementation documents (for `antec`, in `antec/README.md`): value layout (except §22.3), stack size, accuracy of transcendental functions, the drop order of collection elements, hash seeds, the constant-evaluation step limit, limits on how deeply source code may nest (exceeding them is E0216), and the text of diagnostics and of I/O errors. Services adds: the task scheduler and task stack size (§24.1, §24.5.4), TLS cipher suites (§28.3), the bundled time zone database version (§26) and the order of log fields. Web adds: generated image formats and widths (W.18) and the internal structure of generated HTML, CSS and client code beyond W.5. Python adds: the bundled typeshed version, how the asyncio loop thread is run (P.16), the wording of learner hints (P.12.4) and the cost of a bridge operation.

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
| `nijame(sharathu: ato_ito, message: padham ni = "") -> khali` | trap T07 if `kaadhu` ("it's true!"). Always enabled. Usage: `(x > 0) nijame()` |
| `kompa_munigindhi(message: padham ni = "") -> raadhu` | trap T05 ("the house has sunk") |
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
- `a ni, b tho s.byte_mukka() -> sare_thappu[padham, Utf8Thappu]`: copies bytes `a` up to (not including) `b`; `thappu` if a bound isn't on a character boundary
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
| `lekkalu` (math) | the §18.2 families; `sqrt sin cos tan exp log abs min max pow floor ceil round`; `PI E INF NAN`; `lekkalu.motham(xs)` (sum) and `a ni, b tho lekkalu.dot()` over `&mukka[amsam32/64]` using a documented blocked order (vectorizable without `@twaraga`); `lekkalu.polchu_amsam(a, b) -> Kramam` (IEEE totalOrder) |
| `file` | `p ni file.chadhuvu() -> sare_thappu[padham, FileThappu]` ("read"); `p ki, text ni file.raayi() -> sare_thappu[khali, FileThappu]` ("write"); `p ni file.baitlu_chadhuvu()`, `p ki, b ni file.baitlu_raayi()`; `p ni file.undhaa() -> ato_ito`; `p ni file.folderaa() -> ato_ito`; `p ni file.list_cheyyi() -> sare_thappu[varusa[padham], FileThappu]` (sorted names); `p ni file.folder_cheyyi()` (create, including parents); `p ni file.theesey()` (delete a file); `a ni, b tho file.path_kalupu() -> padham`; `p ni file.path_peru()`, `p ni file.path_folder()`, `p ni file.path_extension() -> undochu[padham]` |
| `system` | `system.args() -> varusa[padham]`; `k ni system.env() -> undochu[padham]`; `system.line_chadhuvu() -> undochu[padham]` (stdin); `system.flush()`; `n ni system.vellipo() -> raadhu` (exit); `prog ni, args tho system.nadupu() -> sare_thappu[Phalitham, SystemThappu]` ("run": waits for the program, where `Phalitham{status: ankam32, stdout: padham, stderr: padham}`); `system.os() -> padham`, `system.arch() -> padham` |
| `memory` | †intrinsic, **`naa_risk` only**: `n ni, a tho memory.allocate() -> *maare khali`, `p ni, n tho, a tho memory.release()`, `p ni, n tho, a tho, m ki memory.reallocate()`, `src ni, dst ki, n tho memory.copy()`. Used to write runtime and collection code in "Ante" itself |
| `samayam` (time) | `samayam.ippudu() -> Kshanam` (monotonic "now"); `k.gadichindhi() -> Kaalam` ("elapsed"); `Kaalam.millisec()`, `.nanosec()` |

`FileThappu` and `ParseThappu` are enums that implement `Choopinchochu`. Every standard library record and enum implements `Choopinchochu`, `XeroxTheeyochu` and `Polchochu`; `Kramam` also implements `VarusaPettochu`, `HashCheyochu` and `CopyKottochu`.

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

Tasks, sharing, `baitlu`, data formats, time zones, cryptography, networking and storage are in Part IX (the Services level). `kitiki` strided views remain deferred (Appendix F).

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
| `antec build [path] [-o out] [--release]` | compile a package or file to a native executable |
| `antec run [path] [-- args]` | run the program; `args` become `system.args()` |
| `antec pariksha` | build and run tests ("exam") |
| `antec fmt [path] [--check]` | canonical formatting; must preserve the AST and evaluation order. `--check` changes nothing and fails if a file isn't formatted |
| `antec check [path]` | check a file, or a package folder (default: the current folder), without generating code |
| `antec migrate` | rewrite code for renamed words and new editions (§23.5) |
| `antec emit (tokens\|ast\|mir\|llvm)` | debugging output |
| `antec web (new\|dev\|build\|check)` | web applications (Annex W.22) |
| `antec python (sync\|stubs\|doctor)` | the Python environment and its bindings (Annex P.3.4) |
| `antec repl` | interactive session (§23.7) |
| `antec jupyter install` | registers the "Ante" Jupyter kernel (§23.7) |

`--release` changes optimization level only, never semantics.

23.2 **`ante.toml`.**

```toml
[package]
name = "naa_project"
version = "0.1.0"
edition = "1.0"
kind = "bin"            # "lib", or "web" (Annex W.3)

[dependencies]
lekka_pustakam = "1.2"

[features]
torch = false           # Annex T
python = false          # Annex P
```

A lockfile `ante.lock` pins exact versions, and builds with the lockfile are reproducible. An optional `[build]` table sets `stack = "64MB"` (main-thread stack size; the default is at least 8 MB), `task_stack = "1MB"` (the stack of every other task, §24.5.4) and `target = "<llvm triple>"`. Web packages also have a `[web]` table (Annex W.3.2), and packages that import Python a `[python]` table (Annex P.3.2).

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

23.7 **Interactive use.** Learners try things one line at a time, so the toolchain provides a session.

23.7.1 `antec repl` starts a session, inside the current package when there is one. Each **entry** is one or more complete logical lines:
- An import or declaration is added to the session's module, and later entries can use it. Declaring an item again replaces it for later entries.
- Statements run as if appended to the body of one long function. Bindings stay across entries, and the borrow checker treats all entries as one body: a value moved in one entry is E0601 in a later one.
- An expression statement whose value isn't `khali` is displayed with `Choopinchochu` (otherwise, its type is shown).
- `?` may be used in any entry. A `thappu` ends the entry and is displayed as `thappu: <e>`.
- A compile error discards the entry.
- A trap ends the entry, not the session: the message is shown, earlier bindings keep their values, and the trapped entry binds nothing.
- The session runs on the MIR interpreter, with the same semantics as a built program.

23.7.2 **Jupyter.** `antec jupyter install` registers an "Ante" kernel in which each cell is an entry.
- A displayed `edhaina` (Annex P) uses Python's display protocol (`_repr_mimebundle_`, `_repr_html_`, `_repr_png_`, `_repr_svg_`, then `str`), so pandas tables and images appear inline.
- matplotlib figures created by a cell are shown when the cell ends.
- Other values are shown as text.

23.7.3 The website playground (M19) runs the same session in the browser, without Python.

---

# Part IX — Concurrency and services

Part IX defines what a long-running program, such as a web server, needs beyond Core: tasks, shared ownership, bytes and data formats, time, cryptography, networking, HTTP, storage, logging and configuration. It is the **"Ante" Services 1.0** conformance level (§1.2). Annex W builds web applications on it.

Every name in Part IX lives in a standard module that a program imports with `thechuko`. No new keywords are introduced. `panchukunna` moves from "reserved" to the Types group (§4.4.2).

## 24. Tasks and sharing

### 24.1 Model

24.1.1 A **task** is a lightweight thread of execution with its own stack. The program starts with one task, which runs `modhalu`. Tasks run concurrently and, on multi-core targets, in parallel. The scheduler is implementation-defined (typically M:N over a pool of OS threads, §20.6), with three guarantees:
- **No blocking leaks.** An operation that waits (I/O, `nidrapo`, channels, `aagu`, `Thaalam.theruvu`) suspends only its own task.
- **Fairness.** A task that never waits must not stop other tasks from running forever. An implementation either preempts tasks or inserts yield checks at loop back-edges and function entries.
- **Determinism is not promised** for the interleaving of tasks. A program whose output depends only on values passed through channels, `aagu` results and locks behaves the same across runs, apart from timing.

24.1.2 Concurrency is **structured**: every task belongs to a team (`Jattu`), and a team's scope doesn't end until all its tasks have finished. There is no way to start a task that outlives the scope that started it. Because of this, a task may borrow from the enclosing scope (§24.3).

24.1.3 There are no `async`/`await` keywords and no "colored" functions: any function can wait, and waiting never needs special syntax. (`async`, `await` and `yield` stay reserved, §4.4.2, and remain E1201.)

### 24.2 Teams and tokens (`task` module)

| Signature | Behavior |
|---|---|
| `task.kalisi[T](f: pani[(&Jattu ni) -> T] ni) -> T` | "together": creates a team, runs `f` with it in the current task, then waits until every task started on the team has finished, and returns `f`'s result |
| `f ni j.pampu() -> Token[T]` with `f: pani[() -> T]`, `j: &Jattu` | "send off": starts `f` as a new task on team `j` and returns a token (like a token at the bank: come back with it to collect) |
| `r.aagu() -> sare_thappu[T, TaskThappu]` | "wait": waits for the task and moves out its result. `thappu(TaskThappu.Kompa(t))` if the task trapped (§24.5), `thappu(TaskThappu.Cancel)` if it was cancelled before it produced a value |
| `r.cancel_cheyyi()` | requests cancellation (§24.6); returns at once |
| `j.anni_cancel_cheyyi()` | cancels every unfinished task of the team |
| `d ni task.nidrapo()` with `d: Kaalam` | "go to sleep" for at least `d`; a cancellation point |
| `task.cancel_ayyindhaa() -> ato_ito` | "has it been cancelled?" for the current task |
| `task.cpu_lu() -> ankam` | the number of tasks that can run in parallel (≥ 1) |

`Token[T]` does not implement `CopyKottochu` or `XeroxTheeyochu`; each result is collected at most once. Dropping a token without calling `aagu` is allowed: the task still runs to completion before the team's scope ends, and its result is dropped then.

```ante
task thechuko

motham_kattu(modhalu_: ankam ni, chivara: ankam tho) -> ankam ante:
    (modhalu_ nunchi chivara varaku) |> 0 ni, ((s, x) => s + x) tho poguchey() icchey

rendu_bhagalu(j: &task.Jattu ni) -> ankam ante:
    a ante (() => 1 ni, 1000 tho motham_kattu()) ni j.pampu()
    b ante (() => 1001 ni, 2000 tho motham_kattu()) ni j.pampu()
    a.aagu().kachithanga() + b.aagu().kachithanga() icchey

modhalu() -> khali ante:
    rendu_bhagalu ni task.kalisi() |> choopinchu()
```

Both halves run at the same time; `kalisi` returns only after both tasks are done.

### 24.3 Sending and sharing: `Pampochu` and `Panchukovochu`

24.3.1 Two marker traits, both derived automatically by the compiler and never implemented by hand outside `naa_risk` (E1301):
- **`Pampochu`** ("can be sent"): a value of the type may be moved to another task.
- **`Panchukovochu`** ("can be shared"): a `&T` may be used by several tasks at once.

24.3.2 Rules:
- Scalars, `padham`, `khali`, `raadhu` and every type in Part IX not listed otherwise are both.
- A tuple, array, record, enum, `varusa`, `pette`, `undochu`, `sare_thappu`, `jodi`, `gumpu` is `Pampochu` (or `Panchukovochu`) when all of its components are.
- `&T` is `Pampochu` when `T` is `Panchukovochu`. `&maare T` is `Pampochu` when `T` is `Pampochu`. Neither reference is `Panchukovochu` unless `T` is.
- Every `pani` is `Pampochu` and `Panchukovochu` (§10.7.4).
- `panchukunna[T]` is `Pampochu` and `Panchukovochu` when `T` is `Pampochu` and `Panchukovochu`.
- `Thaalam[T]`, `Kaluva` ends, `Atomic[T]` and `Token[T]` are `Pampochu` and `Panchukovochu` when `T` is `Pampochu`.
- Raw pointers are neither.

24.3.3 `pampu` requires its closure to be `Pampochu` and its result `T` to be `Pampochu` (E1302, naming the capture that isn't). A closure passed to `pampu` may capture borrows from the scope that called `kalisi`: the borrow checker (§15.4) treats every borrow captured by a task closure as live until `kalisi` returns. Two tasks may hold shared borrows of the same place; a task holding `&maare p` excludes every other use of `p` while it runs, exactly as for a single task.

24.3.4 There are no data races in a program without `naa_risk`: every concurrent access goes through `&T` of a `Panchukovochu` type, `Thaalam`, `Atomic` or a channel.

### 24.4 Sharing and locking

**`panchukunna[T]`** ("shared"): an owned, reference-counted, immutable handle to a `T` on the heap.
- `x ni panchukunna.kotthadhi() -> panchukunna[T]`.
- `.xerox()` makes another handle to the same value (count + 1) in O(1); it implements `XeroxTheeyochu` but not `CopyKottochu`.
- `.` and `*` auto-dereference to `&T` (never `&maare T`).
- The value is dropped when the last handle is dropped. Counting is atomic.
- `p ni, q tho panchukunna.okkatenaa() -> ato_ito`: "is it the same one?" (pointer identity).
- Cycles of `panchukunna` leak; this is documented behavior, not undefined behavior.

**`Thaalam[T]`** ("lock"): a mutual-exclusion lock that owns its `T`.
- `x ni Thaalam.kotthadhi()`.
- `l.theruvu() -> Key[T]` ("open" it, getting the "key"): waits until no other task holds the key. `Key[T]` dereferences to `&maare T`. Dropping the key locks it again. A cancellation point only while waiting.
- Opening a lock that the same task already holds is trap **T12**.
- **Poisoning.** If a task traps (§24.5) while holding a key, the lock becomes poisoned. `theruvu` on a poisoned lock traps **T11**; `l.theruvu_choosi() -> sare_thappu[Key[T], Virigindhi]` ("open after checking") returns `thappu` instead, and `Virigindhi.lopala() -> Key[T]` ("inside") recovers the value and clears the poison.
- A typical shared state is `panchukunna[Thaalam[T]]`.

**`Atomic[T]`**, for `T` an integer type or `ato_ito`. All operations are sequentially consistent.
- `x ni Atomic.kotthadhi()`, `a.chadhuvu() -> T` ("read"), `x ni a.raayi()` ("write"), `n ni a.kalupu() -> T` (add; returns the old value; wraps around on overflow, unlike `+`), `x ni a.marpidi() -> T` (swap: "exchange"), `old ni, new tho a.ilaa_unte_maarchu() -> sare_thappu[T, T]` (compare-and-swap: "if it's like this, change it"; `thappu` carries the actual value).

**`Kaluva[T]`** ("canal"): a channel. Values put in at one end come out at the other, in order.
- `n ni task.kaluva[T]() -> jatha[Pampey[T], Andhuko[T]]` with capacity `n ≥ 0` (0 is a rendezvous channel; a negative `n` is T05). `Pampey[T]` ("sender") implements `XeroxTheeyochu`; `Andhuko[T]` ("receiver") does not.
- `x ni p.pampu() -> sare_thappu[khali, Moosesaaru[T]]`: waits while the channel is full. `thappu(Moosesaaru(x))` ("they've closed it") returns the value if every receiver has been dropped.
- `a.andhuko() -> undochu[T]`: waits for a value; `ledhu` once every sender is dropped and the channel is empty.
- `a.ippude_andhuko() -> undochu[T]`: "receive right now", without waiting.
- An `Andhuko[T]` is iterable (`a lo okko x ki:`), ending when the channel is closed and empty.
- Both waiting operations are cancellation points.

**Waiting for the first of several events.** `vetiki ni task.edhi_mundhu() -> ankam` ("whichever first"), with `vetiki: &mukka[Edhuruchoopu]` ("expectation"), waits until one of the expectations is ready and returns its index. If several are ready, the lowest index wins. Expectations are made by:
- `a.andhuko_ready() -> Edhuruchoopu` on an `Andhuko` (ready when a value or the end is available; the caller then calls `ippude_andhuko`);
- `d ni samayam.timer() -> Edhuruchoopu` (ready after `Kaalam` `d`);
- `r.ready() -> Edhuruchoopu` on a `Token`;
- `task.cancel_ready() -> Edhuruchoopu` (ready when the current task is cancelled).

### 24.5 Traps in tasks

24.5.1 A trap in the task running `modhalu` ends the process exactly as in §20.3.

24.5.2 A trap in any other task ends **only that task**:
- its stack is unwound: locals are dropped in the usual order (§19.3), so keys, files and sockets are released. `vellagane` blocks don't run (§19.4);
- every lock it held a key to becomes poisoned;
- its token's `aagu` returns `thappu(TaskThappu.Kompa(t))`, where `t: TrapVivaram` ("trap details") has `code: padham` (`"T01"`…), `message: padham` and `ekkada: undochu[padham]` ("where": the `file:line:col`);
- the trap message is still written to stderr, prefixed with the task's name if it has one (`j.peruto_pampu(peru, f)` starts a named task).

24.5.3 A trap during unwinding (in a destructor) ends the process with exit status 101.

24.5.4 T08 (stack overflow) and T09 (out of memory) in a task other than the first also end only that task, when the implementation can detect them for that task. Each task has its own stack of an implementation-defined size (at least 256 KB; `[build] task_stack` in `ante.toml` sets it).

### 24.6 Cancellation

Cancellation is cooperative. `cancel_cheyyi` marks a task. The task's next **cancellation point**, or the one it is waiting in, returns early: `nidrapo` returns at once, `andhuko` returns `ledhu`, `pampu` on a channel returns `thappu`, and I/O operations return `thappu` with kind `Cancel`. `edhi_mundhu` is a cancellation point only when one of its expectations is `task.cancel_ready()`, and then returns that expectation's index; without one, it keeps waiting for its other expectations. A task that ignores cancellation runs to completion; `aagu` then returns its value. Cancelling a team member never cancels the team's other tasks.

### 24.7 Targets without threads

On `wasm32` client targets (Annex W) the `task` module is not available (E1401). On targets with one hardware thread, tasks still run concurrently.

## 25. Bytes and data formats

### 25.1 `baitlu`

`baitlu` is a standard type alias for `varusa[aasajeevi ankam8]` ("bytes"). Text and bytes convert explicitly:
- `s.baitlu_ga() -> baitlu` copies the UTF-8 encoding;
- `b ni padham.baitlu_nunchi()` (§21.2) validates UTF-8;
- `b ni padham.baitlu_nunchi_lossy() -> padham` replaces invalid sequences with U+FFFD.

Module `encoding`: `b ni encoding.base64() -> padham` and `s ni encoding.base64_chadhuvu() -> sare_thappu[baitlu, EncodingThappu]` (RFC 4648 standard alphabet with padding); `base64url` / `base64url_chadhuvu` (URL-safe alphabet, no padding); `hex` / `hex_chadhuvu` (lowercase out, either case in); `s ni encoding.url_escape()` and `url_unescape` (RFC 3986 percent-encoding of everything but unreserved characters).

### 25.2 Writing and reading values: `Raayochu` and `Chadhavochu`

25.2.1 Two standard traits describe how a value is converted to and from a **data tree**, independent of any one format:
- **`Raayochu`** ("can be written") — `naa.raayi_cheyyi(w: &maare Raasevaadu ni) -> khali`;
- **`Chadhavochu`** ("can be read") — `chadhuvu_cheyyi(r: &maare Chadhivevaadu ni) -> sare_thappu[idhi, ChadhuvuThappu]`, called as `r ni T.chadhuvu_cheyyi()`.

Both are normally obtained with `@automatic Raayochu, Chadhavochu`. Implementing them by hand is allowed. Formats (§25.3) implement `Raasevaadu` ("writer") and `Chadhivevaadu` ("reader").

25.2.2 **The data tree.** Every value maps to one of: null, boolean, integer (any value of a 64-bit signed or unsigned type), float (64-bit; NaN and infinities are rejected when written, `ChadhuvuThappu.Number`), text, bytes, list, map (text keys, ordered), and these mappings are normative:

| "Ante" type | Data tree |
|---|---|
| integers, `ato_ito`, `amsam*` | integer, boolean, float |
| `padham`, `aksharam` | text (a one-character text for `aksharam`) |
| `baitlu` | bytes (base64 text in JSON) |
| `varusa[T]`, `[T; N]`, `mukka[T]` (write only), `gumpu[T]` | list (`gumpu` in iteration order) |
| `jatha[A, B, …]` | list of fixed length |
| `jodi[padham, V]`, `gajibiji_jodi[padham, V]` | map in iteration order (`gajibiji_jodi` sorted by key when written) |
| `jodi[K, V]` with other keys | list of `[key, value]` lists |
| record | map of its fields, in declaration order |
| `undochu[T]` field | the key is left out for `ledhu`; the value for `undhi(v)` |
| `undochu[T]` elsewhere | null, or the value |
| enum variant without fields | text: the variant name |
| enum variant with fields | map `{"rakam": "<Variant>", "0": …, "1": …}` (positional fields by index) |
| `sare_thappu[T, E]` | `{"sare": …}` or `{"thappu": …}` |
| `khali` | null |
| `panchukunna[T]`, `pette[T]` | the value of `T` |

Pointers, references (when reading), `pani`, `Token`, locks, channels and `Rahasyam` (§27.4) implement neither trait (E1303 at the `@automatic`).

25.2.3 **Attributes on fields and variants.** The grammar allows annotations before a record field and an enum variant (§5.1 as amended by this part: `field ::= annotation* 'andhariki'? NAME ':' type NEWLINE`, and likewise for `variant`). Recognized:
- `@peru "name"`: the key used in the data tree ("name");
- `@lekapothe_kotthadhi`: when reading, a missing key takes the field type's `Kotthadhi` value ("skip if new", a default) — the field type must implement `Kotthadhi` (§25.2.5, E1304);
- `@dhaachu`: never written, and read as its `Kotthadhi` value ("hide").

Reading a record **rejects unknown keys** (`ChadhuvuThappu.Theliyani(key)`, "unknown") unless the record carries `@theliyanivi_vadhiley` ("leave unknown ones"). Rejecting by default stops mass-assignment mistakes (Annex W.14).

25.2.4 **Errors** carry a path to the bad value: `ChadhuvuThappu{daari: padham, vidham: ChadhuvuThappuVidham}` where `daari` looks like `.users[3].email` and `vidham` ("kind") is one of `Missing(key)`, `Theliyani(key)`, `TypeMismatch(kavali, vachindhi)` (two `padham` values: the type "wanted" and what "came"), `Number`, `Text`, `Custom(padham)`. Reading never traps.

25.2.5 **`Kotthadhi`** ("a new one", the default value): a standard trait with `kotthadhi() -> idhi`, called as `T.kotthadhi()`, implemented by numbers (0), `ato_ito` (`kaadhu`), `padham` (`""`), collections (empty), `undochu` (`ledhu`), tuples of `Kotthadhi` types, and derivable with `@automatic Kotthadhi` for records whose fields all implement it.

### 25.3 Formats

| Module | Functions |
|---|---|
| `json` | `x ni json.raayi() -> padham` (compact); `x ni json.andanga_raayi() -> padham` ("nicely": two-space indent); `s ni json.chadhuvu[T]() -> sare_thappu[T, ChadhuvuThappu]`; `json.Viluva` ("value") is an enum `Null`, `Bool(ato_ito)`, `Ankam(ankam)`, `PeddhaAnkam(padham)` (an integer outside the `ankam` range, kept as its digits: "big number"), `Amsam(amsam64)`, `Padham(padham)`, `Varusa(varusa[Viluva])`, `Jodi(jodi[padham, Viluva])` for documents of unknown shape, and implements both traits |
| `form` | `application/x-www-form-urlencoded`: `x ni form.raayi()`, `s ni form.chadhuvu[T]()`. Only records whose fields are scalars, `padham`, `undochu` of those, or `varusa` of those (repeated keys) are allowed (E1305) |
| `query` | the same rules as `form`, for URL query strings |

JSON follows RFC 8259: UTF-8, no comments, no trailing commas, duplicate keys rejected (`ChadhuvuThappu.Duplicate`), nesting deeper than 128 rejected, integers that don't fit the target type rejected, numbers with a fraction or exponent read only into floats. Output escapes `"`, `\`, control characters, U+2028, U+2029, `<`, `>` and `&` (so JSON is safe to embed in HTML).

## 26. Time

Module `samayam` (§21.3) gains wall-clock time and calendars.
- `Kaalam` ("duration"): `n ni Kaalam.nanosec()`, `millisec`, `sec`, `nimishalu` (minutes), `gantalu` (hours), `rojulu` (days, exactly 86 400 s); `+`, `-`, comparison; `k.sec_lu() -> amsam64`; display as `1.5s`, `250ms`, `3m 20s`.
- `Gadiyaram` ("clock reading"): an instant in UTC with nanosecond precision, from `samayam.ippudu_utc()`. `g + Kaalam`, `a - b -> Kaalam`, comparison, `g.unix_sec() -> ankam`, `n ni Gadiyaram.unix_nunchi()`. `g.rfc3339() -> padham` (always UTC, `Z`), `s ni Gadiyaram.rfc3339_chadhuvu() -> sare_thappu[Gadiyaram, SamayamThappu]` (any offset). Implements `Raayochu`/`Chadhavochu` as RFC 3339 text.
- `Date` ("date") and `Time` ("time of day"), and `Zone` ("zone"): `peru ni samayam.zone() -> sare_thappu[Zone, SamayamThappu]` with an IANA name (`"Asia/Kolkata"`); the time zone database is bundled with the toolchain (its version is implementation-defined). `g ni, z tho samayam.ikkada() -> jatha[Date, Time]` ("here"); `t ni, v tho, z tho samayam.gadiyaram_ga() -> sare_thappu[Gadiyaram, SamayamThappu]` fails for local times that don't exist and picks the earlier one for times that occur twice.
- `samayam.ippudu()` (monotonic, §21.3) is the only clock that never goes backwards; use it for measuring.

## 27. Randomness and cryptography

Module `bhadram` ("protection"). Every function is constant-time where the algorithm requires it; algorithms are fixed by this spec so that data written by one implementation is read by another.

### 27.1 Randomness

- `n ni bhadram.random_baitlu() -> baitlu`: `n` bytes from the operating system's cryptographically secure generator.
- `a ni, b tho bhadram.random_ankam() -> ankam`: uniform in `a nunchi b lopu` without modulo bias (T05 if `a >= b`).
- `bhadram.uuid_v4() -> padham`, `bhadram.uuid_v7() -> padham` (RFC 9562; v7 is time-ordered).
- Module `random` provides a fast, seedable, **non-cryptographic** generator (`seed ni random.Generator.kotthadhi()`; `g.ankam(lo, hi)`, `g.amsam()`, `xs ni g.kalapu()`, "shuffle") with a documented algorithm (PCG-64), for simulations and games. It never seeds itself from the system.

### 27.2 Hashing and authentication

- `b ni bhadram.sha256() -> baitlu`, `sha512`, `blake3` (32-byte output).
- `msg ni, key tho bhadram.hmac_sha256() -> baitlu`.
- `a ni, b tho bhadram.okkatenaa() -> ato_ito` ("equal?"): constant-time comparison of two byte sequences (lengths may differ; the time depends only on the length of `a`).

### 27.3 Passwords

- `p ni bhadram.password_hash() -> padham`: Argon2id with memory 19 456 KiB, 2 iterations and parallelism 1 (the OWASP minimum), a 16-byte random salt, in PHC string format (`$argon2id$v=19$m=19456,t=2,p=1$…`). The parameters may only grow in later revisions.
- `p ni, h tho bhadram.password_check() -> sare_thappu[PasswordPhalitham, BhadramThappu]` with `PasswordPhalitham` = `Sare`, `SareKaaniMalliHashCheyyi` ("correct, but rehash": the stored hash uses weaker parameters) or `Thappu`. Comparison is constant-time.
- `p` has type `&Rahasyam[padham]` in both functions (§27.4).

### 27.4 Secrets and encryption

- **`Rahasyam[T]`** ("secret") wraps a value that must not leak. Its `Choopinchochu` writes `***`. It implements neither `Raayochu` nor `XeroxTheeyochu` (copy explicitly with `r.bayatiki_theesuko().xerox()`), so it can't end up in a log line, a JSON response or page state by accident. `x ni Rahasyam.kotthadhi()`; `r.bayatiki_theesuko() -> &T` ("take it out") is the one way to reach the value. Its memory is overwritten with zeros when it is dropped.
- `data ni, key tho bhadram.encrypt() -> baitlu` and `bhadram.decrypt() -> sare_thappu[baitlu, BhadramThappu]`: AES-256-GCM with a random 12-byte nonce prepended, key `&Rahasyam[baitlu]` of exactly 32 bytes (T05 otherwise).
- `n ni bhadram.key_tayaru() -> Rahasyam[baitlu]`: "make a key" of `n` random bytes.

### 27.5 What is not provided

Custom cipher modes, RSA, raw elliptic-curve operations and certificate handling beyond §28.3 are left to libraries. A conforming implementation must not implement the functions above with non-constant-time code where the algorithm's reference requires constant time.

## 28. Networking and HTTP

### 28.1 Addresses and errors

`net.Address` is a host name or IP address with a port. `NetThappu` is an enum: `PeruDorakaledhu` (name not found), `Refused`, `Reset`, `Timeout`, `Cancel`, `Moosesaaru` (closed), `Tls(padham)`, `Limit(padham)`, `Os(padham)`. Every network operation returns `sare_thappu[_, NetThappu]` and is a cancellation point (§24.6). Every operation that can wait has a timeout; defaults are listed with each type.

### 28.2 TCP

- `addr ni net.vinu() -> sare_thappu[Listener, NetThappu]` ("listen"); `l.andhuko() -> sare_thappu[Connection, NetThappu]` waits for the next connection.
- `addr ni net.connect() -> sare_thappu[Connection, NetThappu]` (default timeout 10 s).
- `Connection`: `n ni c.chadhuvu() -> sare_thappu[baitlu, NetThappu]` (up to `n` bytes; empty at end of stream), `b ni c.raayi()` (all of `b`), `c.close()`; read and write timeouts via `d ni c.read_timeout()` / `write_timeout` (default: none for raw TCP).

### 28.3 TLS

`c ni, host tho net.tls_connect()` verifies the server certificate against the operating system's trust store and the host name. `net.tls_vinu(addr, cert_file, key_file)` serves TLS with PEM files. TLS 1.2 and 1.3 only; the cipher suites are implementation-defined but must exclude those that TLS 1.3 and RFC 9325 forbid. Turning verification off is possible only inside `naa_risk:` (`net.tls_connect_verify_lekunda`).

### 28.4 HTTP types (`http` module)

- `Method`: `GET`, `HEAD`, `POST`, `PUT`, `PATCH`, `DELETE`, `OPTIONS`, `Itharam(padham)` ("other").
- `Status`: a record with `code: ankam16`, constants `http.OK` (200) … with the standard reason phrases.
- `Headers`: an ordered multimap with case-insensitive names; `h ni, v tho hs.cherchu()`, `n ni hs.thechu() -> undochu[&padham]`, `n ni hs.anni() -> varusa[&padham]`. Header names and values containing CR, LF or NUL are rejected (`thappu`), so response splitting is impossible.
- `Request`: `method`, `url: Url`, `headers`, `body: Body`, `remote: undochu[net.Address]`; `r.json_chadhuvu[T]()`, `r.form_chadhuvu[T]()`, `r.text_chadhuvu()`, `r.cookie(peru)`, `r.query[T]()`.
- `Response`: `status`, `headers`, `body`; constructors `http.Response.text(s)`, `.json(x)`, `.html_document(d)` (Annex W only), `.redirect(url, status)`, `.file(path)`, `.stream(f)` (a task writing chunks to a `Pampey[baitlu]`).
- `Body` is streamed. Reading it all enforces a size limit (default 1 MiB; `http.Limits`) and returns `NetThappu.Limit` past it.
- `Url` (WHATWG URL parsing): `s ni Url.chadhuvu() -> sare_thappu[Url, UrlThappu]`, fields `scheme`, `host`, `port`, `path`, `query`, `fragment`; building with `p ni u.path_kalupu()`, `k ni, v tho u.query_cherchu()` always escapes. `Url.web_aa()` ("is it web?") is true only for `http` and `https`.
- `Cookie` (RFC 6265bis): `peru`, `viluva`, `max_age`, `path`, `domain`, `secure` (default `avunu`), `http_only` (default `avunu`), `same_site` (default `Lax`). Writing a cookie whose name starts with `__Host-` forces `secure`, `path = "/"` and no `domain`.

### 28.5 HTTP server

`app ni, addr tho http.serve() -> sare_thappu[khali, NetThappu]` serves until the task is cancelled or the process receives SIGTERM/SIGINT (or the platform equivalent), where `app: pani[(Request ni) -> Response] ` is the handler. Normative behavior:
- each request runs in its own task on a team owned by `serve`; a trap in the handler returns **500** with a generic body and logs the trap with a request id (§29.1), and the server keeps running;
- HTTP/1.1 (keep-alive, chunked transfer) is required; HTTP/2 over TLS is required when TLS is configured; HTTP/3 is optional;
- default limits (`http.Limits`, all overridable): header block 16 KiB, body 1 MiB, 100 headers, header read timeout 10 s, request timeout 30 s, idle keep-alive 60 s, concurrent connections 10 000. Exceeding them returns 413, 431 or 408 without calling the handler;
- `HEAD` is answered by running `GET` and dropping the body; `Date` and `Content-Length` are set automatically;
- shutdown is graceful: stop accepting, let running requests finish for up to 30 s (`http.Limits.shutdown`), then cancel them.

Handlers compose as ordinary functions. `http.Router` matches method and path patterns (`"/users/{id}"`, `"/files/{*rest}"`) and is built with `pattern ni, method tho, handler kosam r.cherchu()`; a path with two matching patterns is decided by the most specific pattern (more literal segments first), and two patterns equally specific for one method are rejected when the router is built (T05 with both patterns).

### 28.6 HTTP client

`req ni http.pampu() -> sare_thappu[Response, NetThappu]` ("send") with redirects followed up to 10 times (never from `https` to `http`), a 30 s total timeout, TLS verification on, decompression of `gzip`/`br`, and connection reuse. Convenience: `url ni http.get()`, `x ni, url ki http.post_json()`.

### 28.7 WebSocket and server-sent events

- `req ni websocket.accept() -> sare_thappu[WebSocket, NetThappu]` inside a handler (checks `Origin` against the server's own origin unless `websocket.Options.origins` lists others); `url ni websocket.connect()`.
- `WebSocket`: `x ni ws.pampu()` (text or `baitlu`), `ws.andhuko() -> sare_thappu[undochu[websocket.Message], NetThappu]` (`ledhu` after a close frame), automatic ping/pong every 30 s, maximum message 1 MiB by default.
- `sse.stream(f)` makes a `text/event-stream` response; `f` receives a `Pampey[sse.Event]`.

### 28.8 Email

`message ni, server tho mail.pampu() -> sare_thappu[khali, MailThappu]` sends a `mail.Message{from, to, subject, text, html: undochu[padham]}` through an SMTP server (`mail.Server{host, port, user, password: Rahasyam[padham]}`) with implicit TLS on port 465 or STARTTLS otherwise; plain-text SMTP is refused. Header values are encoded (RFC 2047) and line breaks in them are rejected, so header injection is impossible. Several messages may go over one connection with `mail.connect`.

## 29. Storage, logs and configuration

### 29.1 Logging (`log` module)

- `log.debug(...)`, `log.info(...)`, `log.warn(...)`, `log.error(...)` each take a message `padham ni` and `migatha_perlu` fields of any `Raayochu` type: `"login fail" ni, u.id user_id ga log.warn()`.
- Output goes to stderr: one JSON object per line (`{"samayam": "<rfc3339>", "level": "warn", "message": "…", "fields": {…}}`) in release builds, and a coloured human format in debug builds. `ANTE_LOG=debug|info|warn|error` sets the minimum level (default `info`).
- Inside an HTTP handler every line also carries `request_id`.
- `Rahasyam` values can't be logged (they aren't `Raayochu`).

### 29.2 Configuration (`config` module)

- `config.chadhuvu[T]() -> sare_thappu[T, ChadhuvuThappu]` reads a `Chadhavochu` record from environment variables: field `database_url` reads `DATABASE_URL`; nested records use `__` (`SMTP__HOST`); `undochu` fields are optional; `Rahasyam[padham]` fields read secrets. In debug builds, a `.env` file in the package folder is read first; release builds never read `.env`.
- A program should read its configuration once at startup and fail fast: `modhalu` returning `thappu(e)` prints the error path (§20.2).

### 29.3 SQL (`sql` module)

29.3.1 **Query literals.** A new literal prefix `sql"…"` (lexically like an f-string, §4.5) builds a value of type `sql.Query`. Every `{expr}` inside becomes a **bound parameter**, never text spliced into the query. There is no function that turns a `padham` into a `sql.Query`, except `sql.raw_query(s)` inside `naa_risk:`. SQL injection therefore can't happen in safe code.

```ante
user ante sql"SELECT id, peru, email FROM users WHERE id = {id}" ni db.okati[User]()
```

29.3.2 **Databases.**
- `url ni sql.open() -> sare_thappu[Database, SqlThappu]` with `sqlite:path/to/file.db` (the embedded SQLite engine every Services implementation includes, WAL mode, foreign keys on) or `postgres://…` (a PostgreSQL client with TLS).
- `Database` is a pool, `Pampochu + Panchukovochu`; share it by reference or `panchukunna`.
- `q ni db.run() -> sare_thappu[ankam, SqlThappu]` (rows changed), `q ni db.okati[T]() -> sare_thappu[undochu[T], SqlThappu]` ("one"), `q ni db.anni[T]() -> sare_thappu[varusa[T], SqlThappu]` ("all"), where `T: Chadhavochu` reads each row as a map from column names to values (so a record's field names must match the columns; `@peru` renames).
- `f ni db.transaction() -> sare_thappu[R, SqlThappu]` runs `f(&maare Transaction)` and commits if it returns `sare`, rolls back on `thappu` or a trap.
- Value mapping: integers ↔ INTEGER/BIGINT, `amsam64` ↔ REAL/DOUBLE, `padham` ↔ TEXT, `baitlu` ↔ BLOB/BYTEA, `ato_ito` ↔ INTEGER 0/1 or BOOLEAN, `Gadiyaram` ↔ TEXT (RFC 3339) or TIMESTAMPTZ, `undochu[T]` ↔ NULL.

29.3.3 **Migrations.** `migrations/NNNN_name.sql` files in the package are embedded at build time. `db.migrate()` applies the ones not yet recorded in the table `ante_migrations`, in order, each in its own transaction, and fails if an applied file has changed (its SHA-256 differs).

### 29.4 Files and processes

`file` and `system` (§21.3) are Services modules too. In Services, `file` also offers `p ni file.open()` with streaming reads and writes, and `system.signal_vinu()` returns an `Andhuko[Signal]` for `Terminate`, `Interrupt` and `Hangup`.

### 29.5 Service conformance seeds

The seed cases for Part IX are listed in Appendix C under the `KK` (tasks), `DT` (data formats), `NW` (network and HTTP) and `DB` (storage) prefixes.

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

## P.1 Scope and goals

P.1.1 Annex P lets an "Ante" program use any Python package: the scientific and data stack (numpy, pandas, scipy), machine learning (scikit-learn), plotting (matplotlib), computer vision (OpenCV), AI services and models (OpenAI, Hugging Face), PyTorch's Python ecosystem and the standard library. The program stays an "Ante" program: no Python source appears in it, Python exceptions arrive as `sare_thappu` values, and the ownership rules of §15 still hold for every "Ante" value. **"Ante" Python 1.0** = "Ante" Core 1.0 plus this annex. Where a clause uses tasks (§24) or the data tree (§25.2), it applies when the implementation also provides Services 1.0.

P.1.2 **Goals**, in priority order:
1. **Beginners first.** A learner who knows "Ante"'s call syntax can follow any numpy, pandas, scikit-learn, PyTorch or Hugging Face tutorial: the library names, arguments and results are the library's own, and only the spelling of the call changes. When something goes wrong, the message is in Tinglish and says what to do (P.12.4).
2. **Nothing breaks silently.** Every way a Python call can fail is either a `thappu` the program sees or a trap with Python's traceback.
3. **Safe by default.** Without `naa_risk`, no Python object can reach freed "Ante" memory, and no data race between "Ante" code and Python is possible on "Ante" values.
4. **The libraries work as their documentation says.** Values stay Python objects (arrays stay numpy arrays, frames stay pandas frames), so every method, operator, mask and slice a library documents is available, with no wrapper layer to fall behind.
5. **Fast where it matters.** Vectorized work stays inside the libraries; large arrays cross the boundary without a copy when ownership allows it (P.11).

P.1.3 Everything in this annex lives in the module `python` and in modules under it. The only syntax it uses beyond Core is the type keyword `edhaina` ("anything"), which moves from "reserved" to the Types group (§4.4.2).

## P.2 Overview

```ante
python.numpy ni np ga thechuko
python.matplotlib.pyplot ni plt ga thechuko

modhalu() -> sare_thappu[khali, python.PythonThappu] ante:
    x ante 0.0 ni, 6.28 ni, 100 ni np.linspace()?
    y ante x ni np.sin()?
    x ni, y ni plt.plot()?
    "sine.png" ni plt.savefig()?
    big ante y[y > 0.5]
    f"0.5 kanna ekkuva: {big.size}" ni choopinchu()
    sare(()) icchey
```

- `np.linspace` and `plt.plot` are Python functions. Their signatures come from the packages' type stubs (P.6), so a misspelled name or an unknown keyword is a compile error.
- Each call returns `sare_thappu[_, python.PythonThappu]`, and `?` passes a Python exception on (P.12).
- `x`, `y` and `big` have type `edhaina`: handles to numpy arrays. `y > 0.5` is numpy's comparison, giving a boolean mask, and `y[mask]` is numpy indexing (P.8, P.9).

## P.3 The Python environment

P.3.1 **Versions.** The bridge works with CPython 3.11 or later. An implementation links only CPython's Stable ABI (the Limited API of 3.11, which includes the buffer protocol), so one built program runs with every later CPython. On free-threaded CPython builds (3.15t and later) it uses the free-threaded Stable ABI (`abi3t`, PEP 803). An implementation may also use version-specific API when it checks the running version first. Other Python implementations (PyPy, GraalPy) are not required.

P.3.2 **Manifest.** A package that imports Python sets `features.python = true` and may add a `[python]` table:

```toml
[features]
python = true

[python]
version = ">=3.12"                  # accepted CPython versions (PEP 440 specifier)
venv = ".venv"                      # the environment, relative to the package root (default ".venv")
requires = [                        # PEP 508 requirements
    "numpy>=2.1",
    "pandas>=3.0",
    "scipy>=1.14",
    "scikit-learn>=1.5",
    "matplotlib>=3.9",
    "opencv-python-headless>=4.10",
    "openai>=2.0",
    "transformers>=5.0",
]
```

P.3.3 **Finding the environment.** At build time and at run time, the first of these that exists is used:
1. the environment variable `ANTE_PYTHON` (a Python executable, or an environment directory);
2. at run time only: the environment recorded when the program was built, then a bundle next to the executable (P.19.3);
3. the `[python] venv` directory of the package;
4. the active virtual environment (`VIRTUAL_ENV`).

If none exists, `antec` reports E1502 at build time, and the built program stops at start with T14 (P.18.5). The interpreter's version must satisfy `[python] version` (E1502 / T14 otherwise).

P.3.4 **Commands** (§23.1):

| Command | Action |
|---|---|
| `antec python sync` | creates the environment if it is missing (with `uv` when it is on `PATH`, otherwise `python3 -m venv`; when no accepted interpreter is installed and `uv` is available, `uv python install`), installs `requires`, records the interpreter version, its ABI (GIL or free-threaded) and every installed distribution with its version and hash in `ante.lock`, then regenerates the bindings (P.6) |
| `antec python stubs` | regenerates the bindings only |
| `antec python doctor` | prints the interpreter path, version and ABI, each required package and its version, and for each imported module whether it has stubs |

Installation from `ante.lock` verifies the recorded hashes. Builds with an unchanged lockfile produce the same bindings (§23.2.2).

## P.4 Importing

P.4.1 Python modules appear under the module path `python`, with their dotted names (§7.3):

```ante
python.numpy ni np ga thechuko
python.matplotlib.pyplot ni plt ga thechuko
python nunchi cv2, json thechuko
python.openai nunchi OpenAI thechuko
```

An import is valid only with `features.python = true` (E1501). The module must exist in the environment at build time (E1502).

P.4.2 **When imports run.** Before `modhalu` starts, the runtime starts the interpreter (P.17) and imports every Python module the program names, in the order of their first appearance in the program's modules (a deterministic order). A failed import stops the program with T14 and Python's traceback. There is no lazy import at an arbitrary later point, so a missing package never surfaces in the middle of a run. `name ni python.import_cheyyi() -> sare_thappu[edhaina, PythonThappu]` imports a module chosen at run time.

P.4.3 **The package's own Python code.** The directory `python/` at the package root, if present, is put first on `sys.path`. Small Python helpers (for example a subclass a library requires, P.22) live there as ordinary `.py` files and are imported like any other module. There is no way to embed Python source text in "Ante" code.

P.4.4 **Names.** A Python name that is an "Ante" keyword gets a trailing `_` (`lo` → `lo_`, `pettu` → `pettu_`). Python names that aren't valid `NAME`s (non-ASCII identifiers) are reached with `python.attr` (P.8.4).

## P.5 The type `edhaina`

P.5.1 `edhaina` ("anything") is a handle to a Python object: a strong reference. Dropping the handle releases the reference. Python's garbage collector owns the objects; the handles held by "Ante" values keep them alive.

P.5.2 `edhaina` implements `XeroxTheeyochu`: `e.xerox()` makes another handle to the **same** object in O(1), like Python assignment. To copy the object itself, call its Python method (`a.copy()`, `copy.deepcopy`). It doesn't implement `CopyKottochu`, `Polchochu`, `VarusaPettochu` or `HashCheyochu`; its `Choopinchochu` shows `str(obj)` (if `__str__` raises, it shows `<str() failed: Type: message>` and never traps). `e.repr_ga() -> padham` gives `repr(obj)` with the same fallback.

P.5.3 `edhaina` is `Pampochu` and `Panchukovochu` (§24.3): tasks may share Python objects. A Python object's own state is guarded by Python's rules (the GIL, or per-object locking on free-threaded builds), not by §24.3.4. Using one Python object from several tasks at once is therefore not undefined behavior in "Ante" terms, but a library that isn't thread-safe may raise or give inconsistent results.

P.5.4 Python objects are mutable through any handle. The borrow checker governs the **handle**: `&edhaina` and `&maare edhaina` restrict reassigning the variable, not what Python does to the object. Every bridge operation (call, attribute, index, operator) takes the handle by shared borrow.

P.5.5 **Bridge methods.** These names on an `edhaina` are "Ante" methods, found before any Python attribute of the same name: `xerox`, `repr_ga`, `nijamaa`, `okkokkati`, `aagu`, `async_okkokkati`, `varusa_ga`, `mukka_ga`, `maare_mukka_ga`, `idhenaa`. A Python attribute with one of these names is reached with `python.attr` (P.8.4).

## P.6 Signatures from stubs

P.6.1 **Bindings.** For each imported module, `antec python sync` generates "Ante" declarations (the bindings) for the module's functions, classes and annotated attributes. It takes signatures from, in order:
1. the package's own types (PEP 561 `py.typed`, inline annotations or `.pyi` files);
2. a stub distribution for it (`pandas-stubs`, `scipy-stubs`, `types-*`), when one is installed;
3. typeshed, for the standard library (bundled with `antec`);
4. otherwise, runtime introspection (`inspect.signature`) of each public callable. Every type is then `edhaina`, and W1501 is reported once per module.

The bindings live in `target/python/` and are regenerated when `ante.lock` changes.

P.6.2 **Parameters** become formals by kind (Annex T.6 uses the same rule):

| Python parameter | Formal | Passed as |
|---|---|---|
| positional-only (before `/`) | `ni`, in declaration order | `x ni` only (`ga` is E0505) |
| positional-or-keyword | `ni`, in declaration order, with its name as a label | `x ni` or `x name ga` |
| keyword-only (after `*`) | labelled | `x name ga` only (positional is E0505) |
| `*args` | `migathavi` | extra `ni` phrases, or a spread `...xs` |
| `**kwargs` | `migatha_perlu` | extra labels, or a spread of a `jodi[padham, _]` |

A parameter with a default may be left out; Python then applies its own default (the bridge never passes a value for an omitted parameter). A missing required parameter is E0501, an unknown label E0504 (§11.2).

P.6.3 **Types.** An annotation maps to an "Ante" type when it is one of these, and to `edhaina` otherwise (classes, `Any`, `TypeVar`s, protocols, `numpy.typing.NDArray`, unions other than with `None`):

| Python annotation | "Ante" type |
|---|---|
| `int` | `ankam` |
| `float` | `amsam64` |
| `bool` | `ato_ito` |
| `str`, `Literal["a", …]` | `padham` |
| `bytes` | `baitlu` (Services); `edhaina` otherwise |
| `None` | `khali` |
| `X \| None`, `Optional[X]` | `undochu[X]` |
| `list[X]`, `tuple[X, ...]`, `Sequence[X]` | `varusa[X]` |
| `tuple[A, B, …]` | `jatha[A, B, …]` |
| `dict[K, V]`, `Mapping[K, V]` | `jodi[K, V]` |
| `set[X]`, `frozenset[X]` | `gumpu[X]` |
| `Callable[[A, …], R]` | `pani[(A ni, …) -> R]` (P.15) |

- **Results** of a mapped type are converted (P.10). A result that doesn't convert is `thappu(PythonThappu)` with `TypeError` and a message naming the stub, so a wrong stub never becomes wrong data.
- **Parameters** accept any value that converts to Python (P.10), whatever the annotation. Stubs are often narrower than the code. A value whose type clearly contradicts the annotation (`padham` for an `int`) is warning W1502.
- **Overloads** (`@overload`): a call is accepted if it matches some overload's parameters. The result type is mapped only when every matching overload maps to the same type, and is `edhaina` otherwise.
- **Module attributes** with annotations are read with their mapped type, each time they are used (Python modules can change them).

P.6.4 Static checking stops at module-level names: a module's functions, classes and attributes are checked against the bindings (E1503 for a name the bindings don't have). Values are `edhaina`, so methods and attributes **of objects** are looked up at run time.

## P.7 Calls

P.7.1 **Every call into Python returns `sare_thappu[R, PythonThappu]`**, where `R` is the mapped result type (P.6.3) or `edhaina`. A Python exception becomes `thappu` (P.12). This covers functions, classes (constructors), methods of objects and calls of callable `edhaina` values.

P.7.2 **Calling a binding** uses the formals of P.6.2:

```ante
a ante (3, 4) ni np.zeros()?                                    # the shape, positionally
b ante (3, 4) ni, np.float32 dtype ga np.zeros()?               # a keyword argument
c ante "data.csv" ni, ";" sep ga, ["peru", "vayasu"] usecols ga pd.read_csv()?
```

P.7.3 **Calling an object's method or an `edhaina` value** has no static signature. Phrases with role `ni` are positional arguments, in source order; labelled phrases (`x name ga`) are keyword arguments. Any other role is E1507. Spreads pass `*args` (a `varusa`, `mukka` or iterable `edhaina`) and `**kwargs` (a `jodi[padham, _]`).

```ante
m ante a.mean()?
s ante 1 axis ga a.sum()?
r ante (2, 6) ni b.reshape()?
```

P.7.4 **Arguments.** Phrases are evaluated in source order (§11.5); then each is converted to a Python object (P.10) and the call is made. An owned value is consumed (converted, then dropped); a borrowed value is copied into the new Python object; an `edhaina` passes its object. No "Ante" memory is shared with Python by a call, except through P.11.

P.7.5 Constructors are calls: `OpenAI()?`, `"sentiment-analysis" ni pipeline()?`.

## P.8 Attributes and indexing

P.8.1 **Reading** `e.name` gets the attribute. A Python exception in it is trap T13. **Writing** `e.name lo v pettu` sets it (T13 on exception).

P.8.2 **Indexing** `e[k]` calls `e.__getitem__`, and `e[k] lo v pettu` calls `e.__setitem__` (T13 on exception). The key is built from the index expressions:

| In the brackets | Python key |
|---|---|
| one expression | that value, converted (P.10) |
| several, `e[i, j]` | a tuple of them |
| `a nunchi b lopu` | `slice(a, b)` (both ends half-open, as in Python) |
| `_` | `slice(None)`, the whole dimension (`:`) |
| `python.Ellipsis` | `...` |
| `start ni, stop ni, step ni python.slice()` | `slice(start, stop, step)`; pass `python.None` for a missing part |

`a nunchi b varaku` is E1508 in a Python index: Python's negative indices would make the inclusive end change meaning.

```ante
row ante img[0]                       # first row
ch ante img[_, _, 2]                  # channel 2 of an H×W×3 image
mid ante xs[10 nunchi 20 lopu]
evens ante xs[0 ni, python.None ni, 2 ni python.slice()]
adults ante df[df["vayasu"] >= 18]
df.loc[0, "peru"] lo "Ravi" pettu
```

P.8.3 **Compound assignment.** `e += v` (and `-=` `*=` `/=`) calls Python's in-place operator (`operator.iadd`) and stores its result back into `e`, so a numpy array is updated in place. `e[k] += v` and `e.name += v` evaluate `e` and `k` once.

P.8.4 **Functions for the rest** (all return `sare_thappu[_, PythonThappu]` except where noted):

| Function | Python |
|---|---|
| `e ni, "name" tho python.attr()` | `getattr(e, name)` |
| `e ni, "name" ni, v tho python.attr_pettu()` | `setattr(e, name, v)` |
| `e ni, "name" tho python.undhaa() -> ato_ito` | `hasattr(e, name)`; never fails |
| `e ni, k tho python.index_theesey()` | `del e[k]` |
| `a ni, b tho python.okkate() -> ato_ito` | `a is b`; never fails |
| `python.None`, `python.Ellipsis` | the constants (type `edhaina`) |

P.8.5 **Checked forms.** Everything that traps T13 has a form in `python.choosi` ("after checking") that returns `sare_thappu` instead: `python.choosi.attr`, `python.choosi.attr_pettu`, `python.choosi.index`, `python.choosi.index_pettu`, the operators of P.9, and iteration (P.13).

## P.9 Operators

P.9.1 When at least one operand has type `edhaina` and the other converts to Python (P.10), an operator calls Python's operator and the result has type `edhaina`:

| "Ante" | Python | Checked form in `python.choosi` |
|---|---|---|
| `+ - * / % @` | `+ - * / % @` | `kalupu` `theesey` `guninchu` `panchu` `migulu` `matmul` |
| unary `-`, `~` | `-x`, `~x` | `thippu`, `invert` |
| `& \| ^ << >>` | the same | `bit_and` `bit_or` `bit_xor` `lshift` `rshift` |
| `== != < <= > >=` | rich comparison | `eq` `ne` `lt` `le` `gt` `ge` |

A Python exception in an operator is trap T13, like a shape mismatch in Annex T.3.

P.9.2 **Comparisons give `edhaina`,** an exception to §10.5: Python's comparisons need not return a boolean (numpy and pandas return masks). To use a Python value as a condition, convert it: `e.nijamaa() -> sare_thappu[ato_ito, PythonThappu]` calls `bool(e)` (numpy raises for arrays of more than one element, which arrives as `thappu`). `!`, `&&` and `||` need `ato_ito` and don't accept `edhaina` (E0401).

P.9.3 `python.pow(a, b)` (`**`) and `python.floordiv(a, b)` (`//`) are calls and return `sare_thappu`.

## P.10 Conversions

P.10.1 Two standard traits connect "Ante" values to Python objects:
- **`Pythonki`** ("to Python"): `naa.python_ga() -> edhaina` makes a new Python object from the value. It doesn't fail (running out of memory is T09).
- **`Pythonnunchi`** ("from Python"): `e ni T.python_nunchi() -> sare_thappu[T, PythonThappu]`. A value of the wrong Python type is `TypeError`, and an integer out of range is `OverflowError`.

P.10.2 **Built-in implementations:**

| "Ante" type | To Python | From Python, accepts |
|---|---|---|
| `ankam*`, `aasajeevi ankam*` | `int` | `int`, and objects with `__index__` (numpy integers); not `bool` |
| `amsam32`, `amsam64` | `float` | `float`, `int`, objects with `__float__` (numpy floats); `amsam32` rounds to nearest |
| `ato_ito` | `bool` | `bool`, `numpy.bool_`; no truthiness |
| `aksharam` | one-character `str` | a `str` of length 1 |
| `padham` | `str` | `str` |
| `baitlu` (Services) | `bytes` | `bytes`, `bytearray`, `memoryview` (copied) |
| `khali` | `None` | `None` |
| `undochu[T]` | `None` or the value | `None` → `ledhu`, else `undhi(T)` |
| `jatha[A, B, …]` | `tuple` | `tuple` or `list` of that length |
| `[T; N]`, `varusa[T]`, `&mukka[T]` | `list` (copied) | any iterable; for numeric `T` a buffer is read directly (P.11.1) |
| `jodi[K, V]` | `dict`, in insertion order | any `Mapping` |
| `gumpu[T]` | `set` | `set`, `frozenset` |
| `edhaina` | itself | itself (always succeeds) |
| `&T` of any of these | as for `T` (copied) | — |

Other types implement neither trait: records, enums, `sare_thappu`, `pani` (use P.15), iterators, references to other types, `Rahasyam` (§27.4: pass `r.bayatiki_theesuko()` explicitly), locks and channels. Passing one to Python is E1504. The two traits can't be implemented by hand outside `naa_risk`; records and enums cross through the data tree (P.10.3).

P.10.3 **Records and enums** (Services): any `Raayochu` value converts with `x ni python.raayi() -> edhaina`, and any `Chadhavochu` type reads with `e ni python.chadhuvu[T]() -> sare_thappu[T, ChadhuvuThappu]`, using the data tree of §25.2.2 with Python's types: null ↔ `None`, map ↔ `dict`, list ↔ `list` (reading also accepts `tuple`), and the scalars of P.10.2. Floats keep NaN and infinities in both directions. When reading, an object with a `model_dump` method (a Pydantic model, such as an OpenAI response) is read through `model_dump(mode="python")`, a dataclass through `dataclasses.asdict`, and a numpy scalar through `.item()`. The rules of §25.2.3 (unknown keys rejected, `@peru`, `@lekapothe_kotthadhi`) apply.

## P.11 Arrays and buffers

P.11.1 **Copying out.** `e.varusa_ga[T]() -> sare_thappu[varusa[T], PythonThappu]` copies the elements of a Python object into a new `varusa[T]`, for `T` an integer or float type or `ato_ito`. If the object exports a buffer (PEP 3118: numpy arrays, `bytes`, `array.array`, `memoryview`, OpenCV images, CPU tensors through `.numpy()`), its elements are copied in C (row-major) order whatever their strides, and the buffer's element format must be exactly `T`'s (`'d'` for `amsam64`, `'f'` for `amsam32`, `'q'`/`'l'` for 64-bit integers, `'B'` for `aasajeevi ankam8`, `'?'` for `ato_ito`, and so on). A different format is `TypeError` whose message suggests `.astype(...)`; there is no silent cast. Objects without a buffer are iterated and each element converted (P.10). Read the shape from the object itself, for example `e.shape`.

P.11.2 **Moving in, without a copy.** `xs ni python.array_ga() -> sare_thappu[edhaina, PythonThappu]`, for `xs: varusa[T]` with `T` a type of P.11.1, gives a one-dimensional `numpy.ndarray` that takes over the vector's memory: nothing is copied, and the memory is freed by the "Ante" allocator when Python releases the array. The `varusa` is moved, so "Ante" can no longer reach the memory and no race is possible. Reshaping in numpy (`(h, w) ni arr.reshape()`) makes a view, also without a copy. It is `thappu(ImportError)` when numpy isn't installed.

P.11.3 **Borrowed views (`naa_risk`).** `e.mukka_ga[T]()` and `e.maare_mukka_ga[T]()` return `sare_thappu[python.View[T], PythonThappu]` and `sare_thappu[python.MaareView[T], PythonThappu]`. The view dereferences to `&mukka[T]` (or `&maare mukka[T]`) over the object's own buffer, without a copy. The buffer must be C-contiguous with format `T`, and writable for the `maare` form. A view keeps the object alive and holds its buffer export, so the object can't be resized while the view exists. Both calls are allowed only inside `naa_risk` (E1001). The programmer guarantees that while a view exists, no Python code and no other task writes the buffer (for `mukka_ga`), or touches it at all (for `maare_mukka_ga`). Python objects can be reached from code the borrow checker can't see, so this can't be checked.

P.11.4 **PyTorch tensors.** With Annex T also present, `t.python_ga()` on a `torch.Tensor` gives the Python `torch.Tensor` over the same storage, and `e ni torch.Tensor.python_nunchi()` does the reverse. Both use DLPack (version 1.0 or later, negotiated with `max_version`) and copy nothing. Any other DLPack producer (numpy, CuPy, JAX) reaches Annex T the same way.

P.11.5 **Data frames** stay Python objects; there is no "Ante" data-frame type in Python 1.0. Columns come out through `col.to_numpy()` and P.11.1, and rows through `"records" ni df.to_dict()` and P.10.3.

## P.12 Errors

P.12.1 A Python exception, or a failed conversion, gives `PythonThappu`:

```ante
andhariki rakam PythonThappu ante:
    peru: padham          # the exception class, qualified: "ValueError", "openai.RateLimitError"
    message: padham       # str(exception)
    traceback: padham     # Python's formatted traceback; empty when there is none
    object: edhaina       # the exception object itself
```

It implements `Choopinchochu` (`peru: message`) and `XeroxTheeyochu`. `err.idhenaa(cls: &edhaina) -> ato_ito` ("is it this?") is `isinstance(err.object, cls)`, and `kaadhu` if `isinstance` itself fails. Every `BaseException` is converted, including `SystemExit` (Python code calling `sys.exit()` doesn't end the program) and `KeyboardInterrupt`.

P.12.2 `?` works on bridge results in functions returning `sare_thappu[_, PythonThappu]`, or any `E` with `NunchiRavochu[PythonThappu]` (§10.6).

P.12.3 **Trap T13** ("Python lo exception") is raised where §-rules give no `sare_thappu`: attributes, indexing and operators (P.8–P.9), and iteration with `lo okko` (P.13). Dropping or displaying an `edhaina` never traps (P.5.2). The trap message carries the exception's `peru: message` and the traceback, which go to stderr.

```ante
python.openai nunchi RateLimitError thechuko
samayam nunchi Kaalam thechuko
task thechuko

adugu(client: &edhaina, prashna: padham ni) -> sare_thappu[padham, python.PythonThappu] ante:
    0 nunchi 3 lopu okko prayatnam ki:
        r ante "gpt-5.5" model ga, prashna input ga client.responses.create()
        r batti:
            sare(resp) aithe:
                resp.output_text ni padham.python_nunchi() icchey
            thappu(e) aithe:
                e.idhenaa(&RateLimitError) aithe:
                    wait ante (prayatnam + 1) ni Kaalam.sec()
                    wait ni task.nidrapo()
                ledhante:
                    thappu(e) icchey
    thappu("malli malli rate limit" ni python.runtime_error()) icchey
```

`python.runtime_error(message)` (and `python.value_error`, `python.type_error`) build a `PythonThappu` from "Ante" code.

P.12.4 **Hints for learners.** When a T13 trap, or a `thappu(PythonThappu)` returned from `modhalu`, is reported, the report shows the exception's `peru`, `message` and traceback, and the implementation must add a Tinglish hint when it recognizes one of these situations (the hint's wording is implementation-defined):

| Situation | The hint says |
|---|---|
| `ModuleNotFoundError` | which package to add to `[python] requires`, and to run `antec python sync` |
| numpy "truth value of an array … is ambiguous" | use `.any()` or `.all()`, then `nijamaa()` |
| numpy or PyTorch shape mismatch in an operator | the two shapes, read from the operands |
| `AttributeError` on `None` right after `cv2.imread` | the image wasn't loaded: check the path |
| `openai.AuthenticationError` | `OPENAI_API_KEY` isn't set |
| a CUDA device requested where none exists | check `torch.cuda.is_available()` first, or use `"cpu"` |
| pandas `SettingWithCopy`/chained-assignment pattern (P.20.3) | write `df.loc[row, col] lo v pettu` |

## P.13 Iteration

P.13.1 `e lo okko x ki:` iterates any Python iterable: `iter(e)`, then `next` until `StopIteration`, with `x: edhaina`. An exception during iteration is T13. `e.okkokkati() -> python.Iterator` gives the iterator as an "Ante" value implementing `Okkokkati[edhaina]`, so the adapters of §21.4.2 work on it.

P.13.2 **Checked iteration.** `e ni python.choosi.okkokkati()` yields `sare_thappu[edhaina, PythonThappu]` items. After a `thappu` it ends. Use it for streams that can fail midway, such as a streamed model response over the network:

```ante
stream ante "gpt-5.5" model ga, prashna input ga, avunu stream ga client.responses.create()?
stream ni python.choosi.okkokkati() lo okko ev ki:
    ev batti:
        sare(event) aithe:
            event ni choopinchu()
        thappu(e) aithe:
            f"stream aagipoyindhi: {e}" ni choopinchu()
```

P.13.3 "Ante" collections cross into Python as lists (P.10.2). An "Ante" iterator isn't converted lazily (P.22); collect it first with `.varusa_ga()`.

## P.14 Context managers

P.14.1 Python's `with` is `cm ni, f tho python.with_lo() -> sare_thappu[T, PythonThappu]`, with `f: pani[(edhaina ni) -> sare_thappu[T, PythonThappu]]`:
1. it calls `cm.__enter__()` (a `thappu` if that raises), and calls `f` with the result;
2. if `f` returns `sare(v)`, it calls `cm.__exit__(None, None, None)` and gives `sare(v)` (or `thappu` if `__exit__` raises);
3. if `f` returns `thappu(err)`, it calls `cm.__exit__` with `err.object`'s type, value and traceback, so the manager sees the failure (a database connection rolls back), then gives `thappu(err)`. An error is never suppressed, even if `__exit__` returns true.

A trap inside `f` ends the task or process as usual (§20.3, §24.5), and `__exit__` doesn't run (like `vellagane`, §19.4).

```ante
python nunchi sqlite3 thechuko

marks_raayi() -> sare_thappu[khali, python.PythonThappu] ante:
    conn ante "app.db" ni sqlite3.connect()?
    # Commits when the closure gives `sare`, rolls back when it gives `thappu`.
    conn ni, ((c) => "INSERT INTO marks VALUES (1, 95)" ni c.execute()) tho python.with_lo()?
    sare(()) icchey
```

P.14.2 `python.async_with_lo` does the same with `__aenter__`/`__aexit__`, awaited as in P.16.

## P.15 Callbacks

P.15.1 **Owning callbacks.** `f ni python.function_ga() -> edhaina` turns a closure or named function into a Python callable, for `f: pani[(A ni, …) -> R]` where:
- each `A` implements `Pythonnunchi` (often `edhaina`). Python's positional arguments are converted (a mismatch raises `TypeError` in Python), and keyword arguments raise `TypeError`;
- `R` implements `Pythonki`, or is `sare_thappu[R2, PythonThappu]`, where `thappu(err)` raises `err.object` in Python;
- `f` holds no borrow (the borrow checker sees this, §15.4.6), because Python may keep it and call it later from any thread (E1505 otherwise; use P.15.2). Every `pani` is already `Pampochu` and `Panchukovochu` (§10.7.4).

`f ni python.function_ga_anni()` accepts a `pani[(&mukka[edhaina] ni, &jodi[padham, edhaina] tho) -> R]` that receives all positional and keyword arguments ("all"). A closure passed where a stub expects `Callable` is converted with `function_ga` automatically.

P.15.2 **Scoped callbacks.** `python.ikkade[T](body: pani[(&python.Scope ni) -> T] ni) -> T` ("right here") runs `body`. Inside it, `g ni s.function_ga() -> edhaina` accepts closures that **borrow** from the enclosing code. Such a callable works only while `ikkade` is running, only on the task that created it, and only while that task is inside a bridge call. A call at any other time or from another thread raises `RuntimeError("Ante callback ippudu ledhu")` in Python. That covers the usual case (an optimizer or a library calling back during the call itself) without any value escaping its borrow:

```ante
python.scipy.optimize ni opt ga thechuko

# How far the line y = m·x is from the points, for slope `m`.
dooram(m: edhaina ni, xs: &varusa[amsam64] tho, ys: &varusa[amsam64] kosam) -> amsam64 ante:
    slope ante (m[0] ni amsam64.python_nunchi()).kachithanga()
    maare motham ante 0.0
    0 nunchi xs.enni() lopu okko i ki:
        d ante slope * xs[i] - ys[i]
        motham += d * d
    motham icchey

modhalu() -> sare_thappu[khali, python.PythonThappu] ante:
    xs ante [1.0, 2.0, 3.0]
    ys ante [2.1, 3.9, 6.2]
    vethuku ante (s: &python.Scope ni) => (((m) => m ni, &xs tho, &ys kosam dooram()) ni s.function_ga()) ni, [0.0] ni opt.minimize()
    result ante vethuku ni python.ikkade()?
    f"slope = {result.x}" ni choopinchu()
    sare(()) icchey
```

The closure borrows `xs` and `ys`; `ikkade` guarantees scipy can't call it after they are gone.

P.15.3 While "Ante" code in a callback runs, the bridge detaches from the interpreter (P.17.1), so other tasks can use Python. A trap inside a callback can't unwind through Python's C frames: it prints the trap message and ends the process with status 101, as for exported functions (§22.4).

P.15.4 Callbacks can't be pickled. A library that sends a callable to another process (`multiprocessing`, `datasets.map(num_proc=…)`) gets `PicklingError` (a `thappu`). Put such functions in the package's `python/` folder (P.4.3).

## P.16 Async Python

P.16.1 "Ante" has no `async` (§24.1.3). Python awaitables are waited for with `e.aagu() -> sare_thappu[edhaina, PythonThappu]` ("wait"): the bridge runs one asyncio event loop on a thread of its own, started on first use, submits the awaitable to it, and suspends only the current task until it finishes (§24.1.1). Several tasks waiting at once run their awaitables concurrently on that loop. This is how async clients (`AsyncOpenAI`, `httpx.AsyncClient`) give real concurrency:

```ante
python.openai nunchi AsyncOpenAI thechuko
task thechuko

okka_prashna(client: &edhaina, p: &padham ni) -> sare_thappu[edhaina, python.PythonThappu] ante:
    pani_ante ante "gpt-5.5" model ga, p input ga client.responses.create()?
    pani_ante.aagu() icchey

rendu_prashnalu(j: &task.Jattu ni) -> khali ante:
    client ante AsyncOpenAI().kachithanga()
    a ante (() => client, "Telugu lo 'hello' ela cheppali?" ni okka_prashna()) ni j.pampu()
    b ante (() => client, "Binary search ante enti?" ni okka_prashna()) ni j.pampu()
    a.aagu() ni choopinchu()
    b.aagu() ni choopinchu()
```

Both requests are in flight at the same time.

P.16.2 `aagu` is a cancellation point (§24.6): cancelling the task cancels the asyncio task, and `aagu` returns `thappu` with `peru` `"asyncio.CancelledError"`.

P.16.3 `e.async_okkokkati()` iterates an async iterable (`async for`). It yields `sare_thappu[edhaina, PythonThappu]` items, awaiting each `__anext__` as in P.16.1, and ends at `StopAsyncIteration` or after a `thappu`.

P.16.4 Python code that calls `asyncio.run` itself works as any other call (it makes its own loop on the calling thread). The bridge's loop is not the running loop of other threads.

## P.17 Threads, tasks and the interpreter

P.17.1 **Attaching.** There is one interpreter per process; subinterpreters are not used (numpy and many extension modules don't support them). Every bridge operation (call, conversion, attribute, index, operator, iteration step, drop of an `edhaina`) **attaches** the current OS thread to the interpreter for its duration and detaches after: on GIL builds it holds the GIL; on free-threaded builds it holds an attached thread state. "Ante" code between bridge operations never holds the GIL.

P.17.2 **Blocking.** A bridge operation may block its OS thread: Python I/O, waiting for the GIL, a long computation. The scheduler treats every bridge operation as a blocking region, so other tasks keep running (§24.1.1 holds). Cancellation is not observed during a synchronous Python call; it is observed when the call returns.

P.17.3 **Pinning.** From its first bridge operation until it ends, a task stays on one OS thread with one Python thread state. So Python's per-thread state (`threading.local`, `torch.no_grad()`, `decimal` contexts, CUDA streams) behaves as in a Python thread. When a program imports Python, its first task (`modhalu`) runs on the process's **main thread**, which GUI toolkits need (P.20.4).

P.17.4 **Parallelism.** On GIL builds, Python bytecode from different tasks runs one at a time, while work inside extensions that release the GIL (numpy, OpenCV, PyTorch kernels, I/O) runs in parallel. On free-threaded builds, Python code runs in parallel too, and an extension module that doesn't declare free-threading support makes CPython enable the GIL again (CPython's own rule). Which build is used follows from the environment (P.3); a program is correct on both.

## P.18 Output, signals and processes

P.18.1 **Output order.** Before a bridge operation that may run Python code, the runtime flushes "Ante"'s stdout buffer if it holds anything. After it, the bridge flushes `sys.stdout` and `sys.stderr`. Output from "Ante" and from Python appears in program order.

P.18.2 **Signals.** The interpreter is started without Python's signal handlers, so Ctrl-C ends the program as it does without Python (§20). Python code doesn't see `KeyboardInterrupt` from the terminal.

P.18.3 **`sys`.** `sys.argv` is the program path followed by `system.args()`. `sys.executable` is the environment's Python interpreter (not the "Ante" program).

P.18.4 **Child processes.** Before importing any module, the bridge sets `multiprocessing`'s start method to `spawn` on every platform (forking a process that has other threads is unsafe) and its executable to `sys.executable`. Libraries that start worker processes (PyTorch `DataLoader(num_workers=…)`, `datasets.map(num_proc=…)`, `concurrent.futures.ProcessPoolExecutor`) therefore work. Their workers run Python only; what they receive must be picklable (P.15.4).

P.18.5 **Start and end.**
- At start, before `modhalu`, the runtime loads the environment (P.3.3), starts the interpreter and runs the imports (P.4.2). If any of this fails, the program prints the reason and ends with trap **T14** ("Python ready avvaledhu").
- At normal exit (after `modhalu` returns and §19.3 drops), the runtime finalizes the interpreter, running Python's `atexit` handlers.
- On a trap or `system.vellipo(n)`, it only flushes Python's stdout and stderr.

## P.19 Building and deploying

P.19.1 `antec build` doesn't link `libpython`. The built program loads the environment's shared library at start (P.3.3). A program without Python imports never loads it (X001).

P.19.2 Building needs the environment, to read stubs (P.6). `antec check` uses the generated bindings and doesn't start Python unless they are missing.

P.19.3 **Bundling.** `antec build --python-bundle` copies a relocatable interpreter (for example from python-build-standalone) and the locked packages into `<out>.python/` next to the executable. The pair then runs on machines without Python. The `container` deploy target (Annex W.23) includes the environment.

P.19.4 **Targets without CPython.** Importing Python on the `wasm32` client target (Annex W.19) or another target without CPython is E1506.

## P.20 Library notes

P.20.1 These notes show how the rules above meet each library. Each has seed cases in Appendix C (PY-series). The libraries' own documentation applies unchanged; only the spelling of calls changes.

P.20.2 **numpy.**
- Arrays are `edhaina`. Operators, masks, broadcasting and fancy indexing are numpy's (P.8–P.9).
- dtypes are keyword arguments (`np.float32 dtype ga`).
- Random numbers come from `42 ni np.random.default_rng()`.
- **Performance:** each bridge operation costs a few hundred nanoseconds. Keep loops inside numpy (vectorize), or move the data into "Ante" once (P.11.1), loop there, and move the result back without a copy (P.11.2).

P.20.3 **pandas** (3.x).
- Frames and series are `edhaina`, and `df["col"]`, `df.loc[...]` and `df.iloc[...]` are indexing (P.8.2).
- pandas 3 uses copy-on-write: every result of indexing behaves as a copy. Chained assignment (`df["a"][0] lo 1 pettu`) changes nothing, so write `df.loc[0, "a"] lo 1 pettu`.
- Rows come out as records through P.10.3.

P.20.4 **matplotlib.**
- `plt.savefig` works on every task and every machine. Without a display, matplotlib chooses the `Agg` backend itself, and `MPLBACKEND` is passed through.
- Interactive windows (`plt.show()`) need the main task (P.17.3). On macOS a GUI backend raises outside the main thread, which arrives as `thappu`.

P.20.5 **OpenCV** (`cv2`).
- Images are numpy arrays of shape H×W×3 in **BGR** order, `uint8`.
- `cv2.imread` returns `None` for a file it can't read, instead of raising. Check it with `img ni, python.None tho python.okkate()`.
- `cv2.imshow` and `cv2.waitKey` need the main task. On servers, use `opencv-python-headless`.

P.20.6 **scipy.**
- Optimizers, integrators and root finders call back into "Ante" through P.15. Use `python.ikkade` when the function reads local state.
- Sparse matrices are `edhaina`.

P.20.7 **scikit-learn.**
- Estimators are `edhaina`. `fit`, `predict` and `score` take numpy arrays or pandas frames (P.7.3).
- `train_test_split` returns a list of four arrays; index it (`parts[0]`).
- Results such as `accuracy_score` map to `amsam64` from the stubs, so they print and compare as "Ante" numbers.

```ante
python.sklearn.datasets nunchi load_iris thechuko
python.sklearn.model_selection nunchi train_test_split thechuko
python.sklearn.neighbors nunchi KNeighborsClassifier thechuko

modhalu() -> sare_thappu[khali, python.PythonThappu] ante:
    iris ante load_iris()?
    parts ante iris.data ni, iris.target ni, 0.25 test_size ga, 7 random_state ga train_test_split()?
    model ante 3 n_neighbors ga KNeighborsClassifier()?
    parts[0] ni, parts[2] ni model.fit()?
    score ante parts[1] ni, parts[3] ni model.score()?
    f"accuracy: {score}" ni choopinchu()
    sare(()) icchey
```

P.20.8 **OpenAI.**
- The client reads `OPENAI_API_KEY` from the environment. Never pass a `Rahasyam` implicitly (P.10.2).
- Errors are `PythonThappu`; tell them apart with `idhenaa` (P.12).
- Streams iterate with P.13.2 or P.16.3. Many requests at once use tasks with the sync client, or `AsyncOpenAI` and `aagu` (P.16).
- Response objects are Pydantic models, which read into records through P.10.3.

P.20.9 **Hugging Face.**
- `transformers` 5 is PyTorch-only. `pipeline`, tokenizers and models are `edhaina`, configured with keyword arguments.
- `datasets.map(num_proc=…)` uses worker processes (P.18.4) and needs functions from the package's `python/` folder.
- A `Trainer` callback must subclass `TrainerCallback`. Write that small class in `python/`, delegating to an "Ante" function (P.15).
- Hub downloads are cached by `huggingface_hub` as in Python (`HF_HOME`).

```ante
python.transformers nunchi pipeline thechuko

modhalu() -> sare_thappu[khali, python.PythonThappu] ante:
    classify ante "sentiment-analysis" ni pipeline()?
    result ante "Ee movie super undhi!" ni classify()?
    result[0]["label"] ni choopinchu()
    sare(()) icchey
```

- Prefer `safetensors` weights. Loading pickled weights or `trust_remote_code=True` runs code from the model's author, and the bridge doesn't change that.

P.20.10 **PyTorch.**
- Training loops written in "Ante" use Annex T, which needs no Python and no GIL.
- The Python `torch` is for the ecosystem around it (Hugging Face, torchvision transforms).
- Tensors move between the two without a copy (P.11.4).

## P.21 Security

- No string of Python source can be run from "Ante" (no `eval`/`exec` in `python`). Python code lives in reviewed `.py` files (P.4.3). A library that evaluates strings itself (`pandas.eval`, `trust_remote_code`) is the program's choice.
- Secrets reach Python only through `bayatiki_theesuko()` (P.10.2).
- Packages are pinned with hashes in `ante.lock` (P.3.4).
- A Python library runs with the program's full rights. The bridge is not a sandbox.

## P.22 Not in Python 1.0

These are left for later and are rejected with E1201 where syntax would exist:
- subinterpreters;
- defining Python classes in "Ante", or subclassing a Python class (use a helper in `python/`, P.4.3);
- writing Python extension modules in "Ante" (exporting "Ante" functions to Python programs);
- typed instance types generated from stubs (instances are `edhaina`, P.6.4);
- passing an "Ante" iterator to Python lazily, as a generator;
- an "Ante" data-frame type and zero-copy Arrow interchange (`__arrow_c_stream__`).

Interactive use (`antec repl` and the Jupyter kernel, which show Python objects through their `_repr_html_` and `_repr_png_`, so pandas tables and matplotlib figures appear inline) is specified in §23.7.

---

# Annex W — Web applications ("Ante" Web 1.0)

## W.1 Scope

W.1.1 Annex W lets a programmer build and deploy a user-facing web application written only in "Ante". The programmer never writes HTML, CSS or JavaScript: the toolchain generates them. **"Ante" Web 1.0** = "Ante" Services 1.0 (Part IX) plus this annex.

W.1.2 **Goals**, in priority order: (1) safe by default: the secure and accessible way is the only easy way; (2) one language and one mental model from the database to the button; (3) the output is ordinary, standards-based web content that works in every browser of the baseline (W.19.3) and is fast on a cheap phone; (4) the same page code can later be rendered by a native UI back end (W.27).

W.1.3 Everything in this annex is in the module `web` and its submodules (`web.style`, `web.pariksha`), plus the `@server` and `@daari` annotations. No keywords are added.

## W.2 Overview

A web application is:
- a **route** enum (W.10): every URL the application answers;
- a **page** for each route (W.6): state `S`, messages `M`, and three functions — `modhalu` (start), `maarpu` (change) and `roopam` (look);
- optional **server functions** (W.12): ordinary functions marked `@server` that page code calls through commands, and that run only on the server;
- a **theme** (W.9.2), assets (W.18) and translations (W.16);
- the `web.App` value returned by the package's `app()` function (W.11).

```ante
web thechuko

@automatic web.Daari
edho_okati Pagelu ante:      # the routes ("pages")
    @daari "/"
    Intlo                       # "at home"

edho_okati Kaburu ante:        # a message ("what's the news?")
    Penchu
    Thagginchu

rakam Lekka ante:               # the page state ("count")
    viluva: ankam

lekka_modhalu(p: &web.Raaka ni) -> jatha[Lekka, web.Aagna[Kaburu]] ante:
    (Lekka{viluva: 0}, web.emi_ledhu()) icchey

lekka_maarpu(s: &maare Lekka ni, m: Kaburu tho) -> web.Aagna[Kaburu] ante:
    m batti:
        Kaburu.Penchu aithe:
            s.viluva += 1
        Kaburu.Thagginchu aithe:
            s.viluva -= 1
    web.emi_ledhu() icchey

lekka_roopam(s: &Lekka ni) -> web.Node[Kaburu] ante:
    [
        f"Lekka: {s.viluva}" ni web.text(),
        "Penchu" ni, Kaburu.Penchu tho web.button(),
        "Thagginchu" ni, Kaburu.Thagginchu tho web.button(),
    ] ni web.row() |> web.theme.space.m tho web.gap() icchey

lekka_page() -> web.Page ante:
    lekka_modhalu ni, lekka_maarpu tho, lekka_roopam kosam, "Lekka" title ga web.page() icchey

route_page(d: &Pagelu ni) -> web.Page ante:
    d batti:
        Pagelu.Intlo aithe:
            lekka_page() icchey

app() -> web.App ante:
    route_page ni, "Lekka app" peru ga web.app() icchey
```

`antec web dev` serves this at `http://localhost:3000`, and `antec web build --release` produces a deployable application (W.23).

## W.3 Packages

W.3.1 A web package has `kind = "web"` in `ante.toml` and this layout:

```text
ante.toml
src/modhalu.ante       # defines `app() -> web.App` (E1412 if missing)
src/…                  # other modules
assets/                # images, fonts, icons, files served as-is (W.18)
bhasha/<locale>.toml   # translations (W.16), optional
migrations/            # SQL migrations (§29.3.3), optional
```

W.3.2 **`[web]` table** in `ante.toml` (all optional):

```toml
[web]
site_url = "https://ante.dev"       # absolute base URL: canonical links, sitemap, cookies
default_locale = "te"
locales = ["te", "en"]
color_scheme = "system"             # "system" | "light" | "dark"
deploy = "server"                   # "static" | "server" | "container" | "wasi" (W.23)
offline = false                     # W.24

[web.security]                      # W.14; every key relaxes a default and is listed by `antec web build`
connect_src = ["https://api.example.com"]
frame_src = []
img_src = []
cors_origins = []
```

## W.4 One program, two builds

W.4.1 A web package is compiled twice from the same source:
- the **server build**: a native executable (or WASI component) containing the server, the server functions, the server-side renderer and the assets;
- the **client build**: a `wasm32` module containing the code that runs in the browser.

W.4.2 **The client graph** is everything reachable from `app()`'s pages through ordinary references, **excluding the bodies of `@server` functions** (their signatures stay visible). The client graph may not use the modules `task`, `net`, `http` (server side), `websocket` (server side), `sql`, `file`, `system`, `config`, `log`, or the password, encryption and key functions of `bhadram` (E1401). The diagnostic shows the import chain from a page to the offending use. Server-only code goes in `@server` function bodies or in modules only they reach.

W.4.3 Both builds see the same types. A value crosses between them only by being serialized: as page state (W.7.3), as a server function argument or result (W.12), or as a live-mode event (W.20).

W.4.4 The client graph may use: the core language and prelude, `lekkalu`, `samayam` (not `zone` lookups other than UTC and the browser's zone), `encoding`, `json`, `form`, `query`, `random`, `bhadram.sha256`/`random_*`/`uuid_*`, and everything in `web`.

## W.5 The node tree

W.5.1 **`web.Node[M]`** is an immutable description of a piece of user interface whose events produce messages of type `M`. Nodes are semantic: they say what a thing is (a button, a heading, a list), not which HTML tag draws it. The HTML that each node becomes is normative for the web back end (W.5.6), so that assistive technology, search engines and browsers see standard markup.

W.5.2 **Building nodes.** Every constructor is a function in `web`. Text content takes `padham ni`; children take `varusa[web.Node[M]] ni`; the message for an action takes `M tho`; a message built from input takes `pani[(X ni) -> M] tho`. Optional settings are labelled formals with defaults (`…, "en" lang ga`).

**Text and content**

| Constructor | Meaning (HTML) |
|---|---|
| `s ni web.text()` | text inline (`<span>` only when styled; otherwise a text node) |
| `s ni, n tho web.heading()` | heading of level `n` = 1…6 (`<h1>`…`<h6>`); T05 outside 1…6 |
| `parts ni web.paragraph()` | a paragraph of inline nodes (`<p>`) |
| `s ni web.strong()`, `web.emphasis()`, `web.inline_code()`, `web.small()` | `<strong>`, `<em>`, `<code>`, `<small>` |
| `src ni, bhasha tho web.code_block()` | a block of code with syntax highlighting (`<pre><code>`); `bhasha` is `"ante"`, `"text"`, `"json"`, `"sql"`, `"shell"`, `"toml"` or `"rust"`. Highlighting happens at build or render time, never in client JavaScript |
| `items ni web.list()`, `web.numbered_list()` | `<ul>`/`<ol>` of `<li>` |
| `rows ni, header tho web.table()` | a data table (`<table>` with `<thead>`, `<th scope="col">`) |
| `web.divider()` | `<hr>` |
| `asset ni, alt tho web.image()` | an image; `alt` is required text. A purely decorative image uses `asset ni web.design_kosam()` ("just for design", `alt=""`) |
| `asset ni, label tho web.icon()` | an SVG icon with an accessible name; `asset ni web.design_kosam_icon()` hides it from assistive technology |
| `shapes ni, w tho, h kosam, alt ga web.chitram()` | a vector picture ("drawing", inline `<svg role="img">`) from `web.Shape` shapes (W.5.5), for charts and diagrams |
| `src ni, captions tho web.video()` | `<video controls>`; `captions` is a `.vtt` asset, or `web.captions_ledhu("reason")` when the video has no speech |

**Layout and structure**

| Constructor | Meaning |
|---|---|
| `children ni web.column()` / `web.row()` | vertical / horizontal flex layout (`<div>` with flexbox) |
| `children ni web.wrap()` | a row that wraps onto new lines |
| `children ni, cols tho web.grid()` | a grid; `cols` is a `varusa[web.Kolatha]` of track sizes, or `n ni web.columns()` for `n` equal ones |
| `children ni web.stack()` | children drawn on top of each other (for overlays and badges) |
| `web.spacer()` | flexible empty space inside a row or column |
| `child ni web.scroll()` | a region that scrolls when its content overflows, keyboard-focusable |
| `children ni web.header()`, `web.nav(label)`, `web.main()`, `web.footer()`, `web.section(heading)`, `web.article()`, `web.aside(label)` | landmarks (`<header>`, `<nav aria-label>`, `<main>`, …). A page has exactly one `main` (W.15.2) |
| `summary ni, body tho web.disclosure()` | expandable section (`<details><summary>`) |
| `tabs ni, selected tho, on_select kosam web.tabs()` | accessible tab list (WAI-ARIA tabs pattern, arrow-key navigation) |
| `body ni, on_close tho, title kosam web.dialog()` | a modal dialog (`<dialog>` shown modally): focus moves in, is trapped, returns on close; Escape sends `on_close` |
| `s ni web.announce_region()` | a polite live region: screen readers read changes of `s` |
| `web.emi_ledhu_node()` | nothing ("nothing to show") |

**Interactive**

| Constructor | Meaning |
|---|---|
| `label ni, msg tho web.button()` | a button; `label` is its accessible name. `icon ga` adds an icon. Keyboard: Enter and Space |
| `route ni, label tho web.link()` | an internal link to a route value (W.10); never a string |
| `url ni, label tho web.bayati_link()` | an external link ("outside link"): `url: http.Url` with `web_aa()` (else T05); always `rel="noopener noreferrer"` |
| `label ni, value tho, on_input kosam web.text_input()` | single-line text field with a visible `<label>`; `vidham ga` ("kind") is `Text`, `Email`, `Url`, `Phone`, `Search`, `Password` or `Number`; `kavali ga` (required), `error ga` (an `undochu[padham]` shown and announced next to the field), `hint ga`, `autocomplete ga` (an `web.Autocomplete` value; defaults to the right token for the `vidham`) |
| `label ni, value tho, on_input kosam web.text_area()` | multi-line text |
| `label ni, checked tho, on_change kosam web.checkbox()` and `web.toggle()` | `<input type="checkbox">`, the second with `role="switch"` |
| `label ni, options tho, selected kosam, on_change ga web.radio_group()` | radio buttons in a `<fieldset>` with `<legend>` |
| `label ni, options tho, selected kosam, on_change ga web.select()` | `<select>` |
| `label ni, value tho, range kosam, on_change ga web.slider()` | `<input type="range">` |
| `label ni, value tho, on_change kosam web.date_input()` | date picker (`<input type="date">`), value `undochu[samayam.Date]` |
| `label ni, on_pick tho web.file_input()` | file picker; `on_pick` receives `varusa[web.FileHandle]` (read with `web.file_chadhuvu`, W.8) |
| `children ni, on_submit tho web.form()` | a form: Enter submits; the page receives `on_submit` instead of a navigation |

Every interactive constructor requires its accessible name in its signature, so an unlabeled control can't be written. A `label` that is an empty constant is E1410; an empty label at run time is reported by `web.pariksha` checks and in development builds.

W.5.3 **Modifiers** take the node first (`ni`) so that they compose with pipes: `node |> value tho modifier()`.

| Modifier | Meaning |
|---|---|
| `\|> k tho web.gap()` | space between children of a layout node |
| `\|> k tho web.padding()`, `web.padding_x()`, `web.padding_y()` | inner space |
| `\|> a tho web.align()`, `web.justify()` | cross-axis / main-axis alignment (`web.Align.Start`, `Center`, `End`, `Stretch`, `SpaceBetween`) |
| `\|> k tho web.width()`, `web.height()`, `web.min_width()`, `web.max_width()`, `web.min_height()`, `web.max_height()` | sizes |
| `\|> styles tho web.style()` | applies a `varusa[web.style.Style]` (W.9) |
| `\|> msg tho web.on_click()` | for non-button nodes that also have a role from `web.role` (`Button` or `Link`): adds keyboard activation and focusability automatically; plain `<div>` click handlers can't be created |
| `\|> key ni, msg tho web.on_key()` | a key press while the node or a descendant has focus (`web.Key.Enter`, `Escape`, `Arrow*`, `Char(aksharam)`, with `ctrl`/`shift`/`alt`/`meta` flags) |
| `\|> msg tho web.on_focus()`, `web.on_blur()`, `web.on_hover_start()`, `web.on_hover_end()` | focus and pointer-hover messages (hover must not be the only way to reveal information: W.15.3) |
| `\|> payload tho, alternative kosam web.draggable()` and `\|> on_drop tho web.drop_target()` | drag and drop; `alternative` is a message for the single-pointer/keyboard alternative that WCAG 2.5.7 requires |
| `\|> id tho web.gurthu()` | a stable identity ("mark") for focus and scrolling commands (`web.Gurthu`, made with `"name" ni web.Gurthu.kotthadhi()`; unique per page, T05 on duplicates in development builds) |
| `\|> s tho web.tooltip()` | a tooltip shown on hover and on focus, dismissible with Escape |
| `\|> r tho web.role()` | an ARIA role from the closed enum `web.Role`, for composite widgets built from primitives; misuse rules in W.15.4 |
| `\|> lang tho web.lang()` | the language of this subtree (BCP 47) |

W.5.4 **Lists with identity.** `items ni, key tho, roopam kosam web.keyed()` renders `items: &varusa[T]` with `key: pani[(&T ni) -> padham]` and `roopam: pani[(&T ni) -> web.Node[M]]`. The renderer keeps DOM nodes, focus and input state by key across updates. Two equal keys in one `keyed` are T05 in development builds and the later item is dropped in release builds.

W.5.5 **Shapes** for `web.chitram`: `web.Shape` is an enum of `Line`, `Rect`, `Circle`, `Ellipse`, `Path(varusa[web.PathStep])`, `Text`, `Group(varusa[Shape], transform)`, each with fill, stroke and stroke width from the theme's colours. Coordinates are in a `w × h` viewbox. A `chitram` is static; to animate it, change the state.

W.5.6 **Rendering rules.** Text is always escaped. Attribute values are always quoted and escaped. Ids generated for labels, descriptions and error messages are unique within a document. There is no way to insert raw HTML except `s ni web.raw_html()`, an unsafe operation allowed only inside `naa_risk:` (E1001), which the build lists.

W.5.7 **Mapping messages.** `node ni, f tho web.map()` turns a `Node[A]` into a `Node[B]` with `f: pani[(A ni) -> B]`, so a page can embed another page's view (nested components, W.6.4).

## W.6 Pages

W.6.1 `web.page` †intrinsic:

```text
web.page[S, M](
    modhalu: pani[(&web.Raaka ni) -> jatha[S, web.Aagna[M]]] ni,
    maarpu: pani[(&maare S ni, M tho) -> web.Aagna[M]] tho,
    roopam: pani[(&S ni) -> web.Node[M]] kosam,
    title: pani[(&S ni) -> padham],              # required
    description: pani[(&S ni) -> padham] = …,   # default: none
    vinu: pani[(&S ni) -> web.Vinu[M]] = …,     # default: no subscriptions
    rendering: web.Rendering = web.Rendering.Browser,
) -> web.Page
```

It is intrinsic because `web.Page` hides `S` and `M`: one application holds pages with different state types. `title` and `description` may also be given as constant `padham` values (`"Lekka" title ga`), which the compiler wraps.

W.6.2 **Semantics.** A page instance runs this loop, single-threaded, in the browser (or in a server task in live mode):
1. `modhalu(raaka)` gives the first state and a command;
2. `roopam(&state)` is rendered;
3. commands run (W.8); each completed command and each subscription event yields a message;
4. for each message, in arrival order: `maarpu(&maare state, msg)` runs, its command is started, then the view is re-rendered (renders may be coalesced to one per animation frame; `maarpu` is never coalesced or skipped).

`roopam`, `title`, `description` and `vinu` must not have effects; they may be called any number of times. `maarpu` runs exactly once per message. A trap in any of them in the browser shows the error page (W.25) for that page and logs the trap to the console; in development builds it also shows the message and position in an overlay.

W.6.3 **`web.Raaka`** ("arrival": how the user got here) gives `modhalu` the URL, the matched route's raw parameters, the query (`p.query[T]()` with `T: Chadhavochu` per `query` rules, returning `sare_thappu`), the user's locale, and `p.server_lo() -> ato_ito` ("on the server?") for the rare page that must start differently during server rendering.

W.6.4 **Components** are plain functions: a reusable widget is a `(state, message, maarpu, roopam)` set of declarations, embedded with `web.map` in the view and by forwarding messages in `maarpu`. There is no separate component system, no hidden state, and no lifecycle hooks.

## W.7 Rendering modes

W.7.1 `web.Rendering` is `Mundhe` ("beforehand"), `Browser` or `Live`.

| | `Mundhe` | `Browser` (default) | `Live` |
|---|---|---|---|
| HTML first produced | at build time | per request, on the server | per request, on the server |
| Where `maarpu` runs | browser | browser | server task per connection |
| Needs a server at run time | no | only for server functions | yes (WebSocket) |
| Works offline after load | yes | yes (except server calls) | no |
| Client code shipped | only if the page has handlers or subscriptions | yes | a small fixed client, no page code |

W.7.2 **Server rendering settles before sending.** To produce a page's first HTML (at build time for `Mundhe`, per request otherwise), the server runs `modhalu`, then repeatedly runs the **server-function commands** that are pending (W.12) and feeds their result messages to `maarpu`, until none are pending or `web.Options.render_timeout` (default 5 s) passes. Commands that need a browser (timers, storage, focus, clipboard, navigation, external HTTP) are queued and started in the browser after hydration, in their original order. The HTML therefore contains the data, which is good for first paint and for search engines.

W.7.3 **Hydration.** For `Mundhe` and `Browser` pages that ship client code, the settled state `S` is serialized into the document (JSON, escaped per §25.3) together with the queued commands' descriptions. The browser deserializes it, attaches event handlers to the existing DOM without re-creating it, and then runs the queued commands. This requires `S: Raayochu + Chadhavochu` (E1407). Because the state crosses to the browser, it must not contain secrets; `Rahasyam` can't be serialized (§27.4).

W.7.4 **Pages without behavior** (no handler in any node the view can produce for the settled state, no `vinu`) ship no client code in `Mundhe` mode: the result is plain HTML and CSS.

W.7.5 **`Mundhe` routes with parameters.** `web.app`'s `mundhe_routes ga` (a `pani[() -> varusa[Route]]`) lists the parameter values to pre-render; it may call server-only code (it runs at build time on the build machine). A `Mundhe` route value not in that list is rendered on demand like `Browser` if a server exists, else it is the not-found page.

## W.8 Commands and subscriptions

W.8.1 **`web.Aagna[M]`** ("command") describes work to do outside `maarpu`. Commands are values; building one does nothing. The runtime runs them after `maarpu` returns.

| Command | Result message |
|---|---|
| `web.emi_ledhu()` | none ("nothing") |
| `aagnalu ni web.anni()` | runs all, concurrently ("all") |
| `msg ni web.ventane()` | sends `msg` right away, after the current update ("immediately") |
| `d ni, msg tho web.tharvatha()` | sends `msg` after `Kaalam` `d` ("later") |
| `args…, done kosam f.adugu()` | calls server function `f` (W.12); `done: pani[(sare_thappu[T, web.AduguThappu[E]] ni) -> M]` |
| `req ni, done tho web.http_pampu()` | an HTTP request from the browser to an origin allowed by `connect_src` (W.14); `done` receives `sare_thappu[http.Response, web.AduguThappu[khali]]` |
| `route ni web.vellu()` / `web.replace()` | navigate to a route ("go"), pushing / replacing a history entry |
| `web.venakki()` | history back ("backwards") |
| `url ni web.bayatiki_vellu()` | leave the application for an external `http.Url` |
| `id ni web.focus()` | move keyboard focus to the node with `web.Gurthu` `id` |
| `id ni web.scroll_cheyyi()` | scroll the node into view (smooth unless reduced motion is on) |
| `s ni web.copy_cheyyi()` | write text to the clipboard; `done ga` gets `ato_ito` success |
| `name ni, data tho, mime kosam web.download()` | save `baitlu` as a file |
| `h ni, done tho web.file_chadhuvu()` | read a picked file's `baitlu` (limit 64 MiB, `thappu` beyond) |
| `k ni, v tho web.save_cheyyi()` / `k ni, done tho web.saved_thechu()` / `k ni web.saved_theesey()` | persistent per-site storage ("save it"), any `Raayochu`/`Chadhavochu` value, 5 MiB per site, stored as JSON |
| `done ni web.ippudu()` | the current `samayam.Gadiyaram` |
| `a ni, b tho, done kosam web.random()` | a random integer in `a nunchi b lopu` (from `bhadram`) |
| `s ni web.announce()` | a polite screen-reader announcement |
| `mode ni web.color_scheme_maarchu()` | switch between `System`, `Light` and `Dark`, remembered in storage |
| `locale ni web.bhasha_maarchu()` | switch the user's language (W.16), remembered in a cookie |

W.8.2 **`web.Vinu[M]`** ("listen") describes ongoing event sources. After every update the runtime calls the page's `vinu(&state)` and starts or stops sources so that exactly the returned set is active (sources are identified by their constructor and arguments).

| Subscription | Message |
|---|---|
| `web.vinu_ledhu()` / `vs ni web.anni_vinu()` | none / all of them |
| `d ni, msg tho web.prathi()` | `msg` every `Kaalam` `d` ("every") |
| `f ni web.frame()` | `f(Kaalam since last frame)` once per animation frame |
| `f ni web.screen_size()` | `f(web.ScreenSize)` now and on change (`Chinna` < 640 px ≤ `Medium` < 1024 px ≤ `Peddha`, thresholds from the theme) |
| `f ni web.keys()` | key presses anywhere on the page |
| `f ni web.online()` / `web.visible()` / `web.dark_mode()` | connectivity, page visibility, effective colour scheme |
| `url ni, f tho web.websocket()` | messages from a WebSocket (`connect_src` rules apply); `web.Vinu` also provides `ws.pampu` commands through the `web.WebSocketGurthu` it is given |
| `f ni web.route_maarindhi()` | the route changed while staying on this page |

## W.9 Styling

W.9.1 **Model.** A node's look is the theme's defaults for that node kind, then the styles applied with modifiers in order (a later style wins). Styles are typed values; the toolchain turns them into a deduplicated stylesheet of single-purpose classes. There is no selector language, no global cascade to fight, and no way for one page's styles to affect another's.

W.9.2 **Theme.** `web.Theme` is a record of design tokens that every default style refers to:
- `colors`: `primary`, `on_primary`, `surface`, `on_surface`, `muted`, `border`, `danger`, `success`, `warning`, `focus`, each a `web.Color` pair for light and dark schemes (`a ni, b tho web.Color.light_dark()`);
- `space`: `xs`, `s`, `m`, `l`, `xl`, `xxl` (`web.Kolatha`, defaults 4, 8, 16, 24, 32, 48 px);
- `radius`: `s`, `m`, `l`, `full`;
- `font`: `body`, `heading`, `code` (`web.Font`: a family list, ending in a generic family; the default body stack starts with "Noto Sans Telugu" and "Noto Sans" so Telugu and English render well everywhere, bundled with the toolchain as assets);
- `text_size`: `s`, `m`, `l`, `xl`, `xxl`, `display`, in `rem`;
- `breakpoints`: `medium` (640 px), `peddha` (1024 px);
- `motion`: `fast` (120 ms), `normal` (200 ms), easing.

`web.theme` (a constant) is the default theme, readable in code (`web.theme.space.m`). An application passes its own to `web.app` as `naa_theme theme ga`; it is usually made with `web.Theme.kotthadhi()` and record update.

W.9.3 **Values.**
- `web.Kolatha` ("measure"): `n ni web.px()`, `x ni web.rem()`, `x ni web.percent()`, `web.nindu()` ("fill" the available space), `web.saripoyinantha()` ("as much as fits", content size), `x ni web.vw()`, `web.vh()`.
- `web.Color` ("colour"): `"#1e88e5" ni web.Color.hex()` (T05 if malformed), `r ni, g tho, b kosam web.Color.rgb()`, `web.Color.oklch(l, c, h)`, `alpha ga`, plus the theme's tokens. Colours are written to CSS in `oklch` with an sRGB fallback.

W.9.4 **Style properties** (module `web.style`; each returns a `Style`). The CSS each one produces is normative for the web back end.

| Group | Functions |
|---|---|
| Spacing and size | `gap`, `padding`, `padding_x`, `padding_y`, `margin_top`, `margin_bottom` (only vertical margins, for flow content), `width`, `height`, `min_*`, `max_*`, `aspect_ratio` |
| Layout | `align`, `justify`, `grow`, `shrink`, `grid_span` (columns, rows), `order`, `overflow` (`Visible`, `Hidden`, `Scroll`) |
| Colour | `color`, `background`, `border_color`, `opacity` |
| Border and shape | `border` (width, `Solid`/`Dashed`), `radius`, `shadow` (`web.Shadow.S`/`M`/`L` from the theme) |
| Text | `font`, `text_size`, `weight` (100–900), `line_height`, `letter_spacing`, `text_align`, `italic`, `underline`, `truncate` (one line with ellipsis), `line_clamp` (n lines), `no_wrap` |
| Position | `sticky_top` (stays at the top while scrolling), `overlay` (inside a `stack`: `Top`, `Bottom`, `Center` …), `layer` (`web.Layer.Base`, `Raised`, `Overlay`, `Toast` — a closed set instead of arbitrary z-indexes) |
| Motion | `transition` (which properties, a `motion` token), `animation` (`web.Animation` keyframes made from `Style` lists, duration, repetitions), `transform` (`translate`, `scale`, `rotate`) |
| Interaction | `cursor`, `select_none` |
| Variants | `hover(styles)`, `pressed(styles)`, `focus(styles)` (changes the focus look but can't remove the focus ring: W.15.5), `disabled(styles)`, `dark(styles)`, `medium_screen(styles)`, `peddha_screen(styles)` (mobile first: base styles apply to all sizes) |

W.9.5 **Motion safety.** When the user's system asks for reduced motion, every `transition` and `animation` has zero duration and smooth scrolling is off, unless the style is marked `web.style.avasaram_motion()` ("needed motion"), which W.15.6 limits.

## W.10 Routes

W.10.1 The application's routes are an enum with `@automatic web.Daari` ("way"). Each variant carries `@daari "<pattern>"`:
- a pattern starts with `/`; segments are literals, `{name}` (one segment) or a final `{*name}` (the rest of the path);
- the placeholders must correspond, in order, to the variant's fields; each field's type must be an integer type, `padham`, or an enum without fields (matched by variant name in lowercase), else E1405;
- two patterns that can match the same path are E1406, unless one has strictly more literal segments in the first position where they differ (it wins);
- a variant without `@daari` is E1405, except one variant marked `@dorakaledhu` ("not found"), which is used for every unmatched path and answers with status 404.

W.10.2 The derive provides `d.url() -> padham` (percent-encoding fields) and `u ni Daari.chadhuvu() -> undochu[Daari]` (decoding; `ledhu` for no match), with `u.chadhuvu().url()` round-tripping. Trailing slashes are redirected (308) to the form without one. Query strings are never part of the route; pages read them through `Raaka.query`.

W.10.3 Internal links take route values (`web.link`), so a renamed or removed route is a compile error wherever it was linked.

## W.11 The application

`web.app` †intrinsic:

```text
web.app[R: web.Daari, A: Pampochu + Panchukovochu](
    pages: pani[(&R ni) -> web.Page] ni,
    peru: padham,                                  # the site's name
    theme: web.Theme = web.theme,
    state: undochu[panchukunna[A]] = ledhu,        # shared server state (W.12.4)
    mundhe_routes: pani[() -> varusa[R]] = …,      # W.7.5
    session: web.SessionOptions = …,               # W.13
    layout: pani[(&R ni, web.Node[web.Lopala] tho) -> web.Node[web.Lopala]] = …,
    http_routes: http.Router = …,                  # extra plain HTTP endpoints (webhooks, APIs)
) -> web.App
```

`layout` wraps every page's view (header, navigation, footer) and receives the page's node as an opaque `web.Lopala` ("inside") value, so layouts can't send page messages. `http_routes` are matched before page routes, under paths that page routes don't use (E1406 otherwise, checked at startup with T05 for dynamically built routers).

## W.12 Server functions

W.12.1 `@server <guard>` before a function makes it a **server function**: its body is compiled only into the server build, and the client calls it through `f.adugu(…)` ("ask", W.8). The annotation's argument names either a guard function or the identifier `evarikaina` ("anyone") to state explicitly that it is public. A missing argument is E1403. Access control is therefore never forgotten silently (OWASP A01).

```ante
@server login_ayyaru
todo_cherchu(text: padham ni, ctx: &web.ServerCtx[AppState] kosam) -> sare_thappu[Todo, TodoThappu] ante:
    user ante ctx.session().user().kachithanga()
    app ante ctx.state()
    id ante sql"INSERT INTO todos (user_id, text) VALUES ({user}, {text}) RETURNING id" ni app.db.okati[ankam]()?
    sare(Todo{id: id.kachithanga(), text: text}) icchey
```

The guard `login_ayyaru` ("logged in?") has already refused anonymous callers, so `user()` is present; the query's `{user}` and `{text}` are bound parameters.

W.12.2 **Signature rules** (E1402): parameters are `Chadhavochu`, except an optional last formal `ctx: &web.ServerCtx[A] kosam`; the result is `sare_thappu[T, E]` with `T, E: Raayochu`. Every server function in an application uses the same `A` as `web.app`'s `state` (E1414). The client-side `adugu` takes the same phrases minus `ctx`, plus `done kosam`.

W.12.3 **Guards.** A guard is `g(ctx: &web.ServerCtx[A] ni) -> sare_thappu[khali, web.Vaddhu]` (E1404 for another shape). It runs before the arguments are decoded. `web.Vaddhu` ("no") is `LoginKaavali` (→ 401) or `PermissionLedhu` (→ 403, "no permission"). Guards compose: `g1 ni, g2 tho web.rendu_kaapala()` ("both guards").

W.12.4 **`web.ServerCtx[A]`** gives: `ctx.request() -> &http.Request`, `ctx.session() -> &maare web.Session` (W.13), `ctx.state() -> &A` (the application's shared state, e.g. a database pool), `ctx.request_id() -> padham`, and `ctx.cookie_raayi(c)`.

W.12.5 **Transport** (normative, so clients and servers of one build always agree):
- `POST /_ante/server/<id>` where `<id>` is the base64url SHA-256 of the function's module path, name and serialized signature, truncated to 16 bytes;
- body: `{"args": [...]}` in JSON; response `{"sare": …}` or `{"thappu": …}` with status 200;
- the request must have header `X-Ante-Server: 1` and either `Sec-Fetch-Site: same-origin` or an `Origin` equal to the site's origin (403 otherwise). Browsers can't send that header cross-site without a CORS preflight, which the server refuses, so CSRF is prevented without tokens in page code;
- a body that doesn't decode is 400; a trap in the function is 500 with `{"request_id": …}`; an unknown `<id>` (the client is from an older or newer deployment) is 409.

W.12.6 **`web.AduguThappu[E]`** ("asking failed") is what `done` sees on failure: `Mee(E)` ("yours": the function returned `thappu(e)`), `Vaddhu(web.Vaddhu)`, `Network`, `Server(request_id)`, `Version` (409: the runtime reloads the page once and then reports `Version`), `Timeout` (default 30 s).

W.12.7 Server functions run in the server task that handles the request, so they may wait on the database, call other services and use `task.kalisi`.

## W.13 Sessions and login

W.13.1 Every request has a `web.Session`, stored on the server, identified by a random 256-bit id in the cookie `__Host-ante_session` (`Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/`). The store is in memory by default (development), or SQLite or PostgreSQL through `web.SessionOptions.store`. A session that carries no data and no login sets no cookie.

W.13.2 Operations: `s.user() -> undochu[padham]`; `id ni s.login()` (stores the user id and **rotates** the session id, preventing session fixation); `s.logout()` (destroys the session); `k ni, v tho s.daachu()` ("keep") and `k ni s.thechu[T]() -> undochu[T]` for any `Raayochu`/`Chadhavochu` value; expiry after 30 minutes idle and 12 hours absolute by default (`web.SessionOptions`), or 30 days with `avunu remember ga` on `login`.

W.13.3 `web.auth` provides the parts of login that are easy to get wrong:
- password login helpers built on `bhadram.password_hash`/`password_check` (§27.3), including rehashing on `SareKaaniMalliHashCheyyi` and a uniform response time for unknown users;
- `web.auth.limit` ("limit"): a per-key rate limiter (`key ni, n tho, per kosam web.auth.limit()` → `sare_thappu[khali, web.Vaddhu]`) for login and reset endpoints, stored with the sessions;
- `web.auth.oidc`: OpenID Connect sign-in ("Sign in with …") with the authorization code flow, PKCE, `state` and `nonce` checks, and ID-token validation against the provider's published keys;
- `web.auth.reset_token`: single-use, expiring, hashed tokens for password-reset and email-confirmation links.

W.13.4 Email: `mail.pampu(server, message)` (Part IX §28.8) sends mail over SMTP with STARTTLS or TLS.

## W.14 Security defaults

The following are normative for the server build. Each `[web.security]` key that relaxes one is reported by `antec web build`.

1. **Escaping and URLs.** W.5.6; `web.bayati_link`, `web.bayatiki_vellu` and image sources accept only `http.Url` values that are `web_aa()` or same-origin paths, so `javascript:` URLs can't be produced.
2. **Content Security Policy** on every HTML response: `default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'; upgrade-insecure-requests`, extended only by `connect_src`, `frame_src` and `img_src` from `[web.security]`. The generated client never needs `'unsafe-inline'` or `'unsafe-eval'`.
3. **Headers** on every response: `X-Content-Type-Options: nosniff`, `Referrer-Policy: strict-origin-when-cross-origin`, `Cross-Origin-Opener-Policy: same-origin`, `Cross-Origin-Resource-Policy: same-origin`, `Permissions-Policy` denying camera, microphone, geolocation, payment and USB; `Strict-Transport-Security: max-age=63072000; includeSubDomains` when served over HTTPS.
4. **CSRF**: W.12.5 for server functions; `web.form` posts go through server functions too; plain `http_routes` handlers that change state must check `http.same_origin(req)` (a W-lint, W1406, warns on POST/PUT/PATCH/DELETE handlers that never call it).
5. **CORS**: no `Access-Control-Allow-Origin` unless `cors_origins` lists origins; `*` is not accepted with credentials.
6. **Cookies**: §28.4 defaults; session cookie W.13.1.
7. **Input**: server function arguments and `form`/`query` bodies are decoded into types (§25.2), with unknown keys rejected, so mass assignment can't happen; HTTP limits §28.5.
8. **SQL**: only `sql"…"` queries (§29.3.1).
9. **Errors**: in release builds a 500 page or response never contains trap messages, stack positions or `thappu` contents from server functions other than the declared `E` (OWASP A10); the request id links the user's report to the log line.
10. **Secrets**: `Rahasyam` can't reach page state, server function results or logs (§27.4).
11. **Supply chain**: `ante.lock` records a SHA-256 for every dependency; builds fail on a mismatch; `antec web build` prints the dependency list with versions (OWASP A03).
12. **Uploads** (`http_routes` and `web.file_input` → server functions): stored outside `assets/`, never served with a type sniffed from content, size-limited.

## W.15 Accessibility

W.15.1 The web back end must produce output that meets **WCAG 2.2 level AA** for everything the toolkit controls. What the programmer controls is constrained as follows.

W.15.2 **Structure.** `<html lang>` is set from the active locale; every page has exactly one `web.main` (E1411 when a layout and page can be seen to produce zero or two; otherwise checked at run time in development builds and by `web.pariksha`); headings shouldn't skip levels (W1403); a "skip to main content" link is inserted first in every document.

W.15.3 **Names and labels.** Required by constructor signatures (W.5.2); images need `alt` or `design_kosam`; icon-only buttons take their `label` as the accessible name; `error` text is linked with `aria-describedby` and announced; hover-only content is impossible because `tooltip` also opens on focus.

W.15.4 **Roles.** `web.role` accepts only roles whose required states the node can provide; using `Button` or `Link` adds keyboard handling; roles that need managed focus (`Menu`, `Listbox`, `Grid`) are offered only through toolkit widgets, not through `web.role` (E1415).

W.15.5 **Focus.** Every interactive node is reachable with Tab in document order; the focus indicator is at least a 2 px outline with 3:1 contrast against its surroundings, using `theme.colors.focus`, and can be restyled but not removed (`style.focus` may not set the outline to nothing; E1416 for constants); `sticky_top` and overlays reserve scroll padding so focused elements aren't hidden (WCAG 2.4.11); dialogs trap and restore focus.

W.15.6 **Pointer and motion.** Interactive nodes have a minimum target of 24 × 24 CSS px (W1402 when a constant size makes one smaller); drag operations require an alternative (W.5.3); nothing flashes more than three times a second (`animation` with a period under 333 ms that changes colour or opacity is E1417); reduced motion is honoured (W.9.5); `avasaram_motion` is only allowed on `transform` animations shorter than 5 s.

W.15.7 **Colour.** Theme token pairs used for text (`on_primary` on `primary`, `on_surface` on `surface`, …) must have a contrast ratio of at least 4.5:1 in both schemes; constant pairs are checked at compile time (W1401), others by `web.pariksha.a11y`.

W.15.8 **Forms.** Every input has an `autocomplete` token (W.5.2); password fields allow pasting and password managers; authentication never requires a cognitive test (WCAG 3.3.8).

## W.16 Languages

W.16.1 Text shown to users can be written inline, or as keys in `bhasha/<locale>.toml` files:

```toml
# bhasha/te.toml
swagatham = "Swagatham, {peru}!"
items = "{n, plural, one {# item} other {# items}}"
```

`"swagatham" ni, peru ga web.maata()` ("word") returns the text in the active locale, with arguments given as labelled phrases. Messages use the ICU MessageFormat subset `{x}`, `{x, plural, …}` (CLDR plural rules) and `{x, select, …}`.

W.16.2 A key missing from the default locale's file is E1408; missing from another locale is W1404, and falls back to the default locale at run time. Argument names are checked against the message (E1408).

W.16.3 The active locale is the user's choice (a cookie set by `web.bhasha_maarchu`), else the best match of `Accept-Language` among `locales`, else `default_locale`. `web.ankelu` ("numbers") formats numbers, currencies (`INR` by default for `te`/`en-IN`, with Indian digit grouping: 1,00,000) and dates per CLDR.

## W.17 Documents, metadata and search

- Every page has a `title` (required by `web.page`); the document title is `"<page title> · <site peru>"`.
- `description` becomes `<meta name="description">` and the Open Graph description; `og_image ga` (an asset) on `web.page` sets the preview image.
- Canonical URLs use `site_url` (E1418 at build when `site_url` is missing and a page is rendered for production).
- `/sitemap.xml` lists every route without parameters and every value from `mundhe_routes`; `/robots.txt` allows all and points to the sitemap, unless `web.app`'s `robots ga` overrides it.
- The viewport, charset and colour-scheme meta tags are always present.

## W.18 Assets and fonts

- `"logo.svg" ni web.asset()` refers to `assets/logo.svg`; a constant path that doesn't exist is E1409. The returned `web.Asset` knows its URL, MIME type and, for images, its intrinsic size (so the renderer sets `width`/`height` and the layout doesn't shift).
- Asset URLs include a content hash (`/a/logo.3f9c2a1b.svg`) and are served with `Cache-Control: public, max-age=31536000, immutable`; HTML is served with `no-cache`.
- Raster images are converted at build time to modern formats at several widths and served with `srcset`; the exact formats and widths are implementation-defined.
- Fonts listed in the theme are served from the application's own origin (no third-party font hosts) with `font-display: swap`.

## W.19 Client runtime

W.19.1 The client build is a `wasm32` WebAssembly module plus a small loader script generated by the toolchain. Programmers never edit either. The loader is the only JavaScript in the application, apart from libraries' `bayata "js"` code (W.19.4).

W.19.2 Rendering in the browser compares the new node tree with the previous one and applies the minimal DOM changes, keyed by `web.keyed` keys and by position otherwise. Input fields keep their cursor position and IME composition (needed for Telugu input methods) while their value is driven by state.

W.19.3 **Browser baseline**: the browsers that support WebAssembly with bulk memory, sign extension, multi-value and non-trapping float-to-int conversion, ES2020 modules and `<dialog>`, i.e. current versions of Chromium-based browsers, Firefox and Safari, and the two previous major versions of each. WasmGC, JSPI and WebAssembly threads are not required. Pages in `Mundhe` mode without behavior work without WebAssembly and without JavaScript.

W.19.4 **JavaScript interop for library authors.** A library may declare `bayata "js" ante:` functions, implemented in a `.js` file in the package, using only numbers, `ato_ito`, `padham` and `baitlu` as parameter and result types. Calling one is an unsafe operation (`naa_risk`, E1001). Applications never need this; it exists so that new browser APIs can be wrapped safely once and reused.

## W.20 Live mode

W.20.1 A `Live` page's state stays on the server, in a task per browser tab. The first response is server-rendered HTML (W.7.2) with a small fixed client. The client opens a WebSocket to `/_ante/live` (same origin, carrying the session cookie and a signed page token), and from then on:
- user events are sent as `(node path, event kind, payload)`; the server finds the handler in its latest view, makes the message, runs `maarpu`, renders, and sends the DOM patch;
- commands that need the browser (W.8.1) are sent to the client to run; their results come back as events;
- server functions are called directly, without HTTP.

W.20.2 If the connection drops, the client shows a "reconnecting" notice (announced to screen readers), retries with backoff, and on reconnect the server starts the page again from `modhalu` (state is not kept across connections unless the page stores it in the session). The server ends a page task 60 s after its connection closes.

W.20.3 `Live` pages don't need `S` to be serializable. They need a server deployment target (E1413 otherwise).

## W.21 Testing

`web.pariksha` runs pages and applications inside `@pariksha` tests, without a browser:
- `page ni, url tho web.pariksha.modhalu() -> Harness[M]` starts a page (running server functions in-process against the test's app state);
- queries find nodes the way a user would: by role and accessible name (`web.Role.Button ni, "Penchu" tho h.vethuku()`, "search"), by label, by text; a query that matches zero or several nodes is a failure with a printout of the tree;
- actions: `h.click(node)`, `text ni, node tho h.type()`, `key ni h.press()`, `h.submit(form)`; each runs messages to completion, advancing a virtual clock for `tharvatha` and `prathi` (`d ni h.time_munduku()`, "time forward");
- `h.html() -> padham` gives the rendered document for snapshot tests;
- `h.a11y() -> varusa[web.pariksha.Issue]` checks the rendered tree against W.15 (names, roles, contrast of the actual colours, heading order, landmarks, duplicate ids, target sizes);
- `app ni web.pariksha.server()` gives an in-process HTTP client for `http_routes` and server functions, including guards and CSRF rules.

## W.22 Tooling

| Command | Action |
|---|---|
| `antec web new <peru>` | a new web package with a working example, tests and a theme |
| `antec web dev` | builds both sides and serves `http://localhost:3000`; rebuilds on change and reloads the browser. When the page's `S` is serializable, the page's state is kept across reloads (when its type changed, it starts again from `modhalu`). Errors are shown in the browser in an overlay and in the terminal |
| `antec web build [--release]` | builds `dist/` for the configured deploy target (W.23) and prints a report: pages and their rendering modes, client code size per page, relaxed security settings, dependencies |
| `antec web check` | `antec check` for both builds plus the translation, asset, route and accessibility checks |
| `antec pariksha` | runs tests, including `web.pariksha` ones |

## W.23 Deployment

W.23.1 `[web] deploy` selects what `antec web build` produces:

| Target | Output in `dist/` | Requirements |
|---|---|---|
| `static` | a folder of files for any static host: HTML per pre-rendered route, assets, client modules, and `_ante/headers.json` listing the headers of W.14 per path for hosts that support custom headers | no server functions, no `Live` pages, every page `Mundhe` or `Browser` without server functions (E1413) |
| `server` | one native executable with every asset embedded | — |
| `container` | the `server` executable plus an OCI image layout (`dist/image/`) on a minimal base image, running as a non-root user | — |
| `wasi` | a WASI 0.3 component implementing `wasi:http`'s incoming handler, for edge platforms | no `Live` pages; storage only through the platform (SQLite files are unavailable) |

W.23.2 **The server executable** listens on `$PORT` (default 8080) on all interfaces, reads configuration from the environment (§29.2), applies migrations on start when `migrate_on_start` is set, answers `GET /_ante/health` with 200 once ready, logs JSON lines (§29.1), and shuts down gracefully on SIGTERM (§28.5). It serves plain HTTP; TLS is expected to be terminated by the platform's proxy, or configured with `ANTE_TLS_CERT`/`ANTE_TLS_KEY`.

W.23.3 `antec` doesn't contain code for any particular hosting company. Deploying is copying `dist/` to a static host, running the executable on a server, pushing the image to a registry, or uploading the component. The documentation (not this spec) gives recipes for common platforms.

## W.24 Offline

With `offline = true`, the build adds a service worker that caches the application shell, client modules and assets, so `Mundhe` and `Browser` pages open without a network; server-function commands then fail with `Network`. Updates are detected on the next visit and activated on the following page load, never in the middle of a session.

## W.25 Errors and status pages

- An unmatched path renders the `@dorakaledhu` route's page with status 404 (a built-in page if there is none).
- A trap while rendering on the server gives status 500 and the built-in error page, in the user's locale, with the request id; release builds show no internals.
- A trap in browser-side page code replaces that page's view with the same error page and keeps the layout, so navigation still works.

## W.26 Not in Web 1.0

Drawing with a pixel canvas, WebGL and WebGPU; camera, microphone and screen capture; geolocation; push notifications; payments; Bluetooth/USB/serial; WebRTC; passkeys (WebAuthn) as a built-in (possible through a library with `bayata "js"`); server-streamed partial HTML (the first HTML is sent whole); code splitting below page level. These are listed in Appendix F.

## W.27 Native user interfaces (later)

`web.Node`, pages, commands and subscriptions don't mention HTML. A later annex (Annex N, native desktop and mobile) will render the same pages with the platform's own widgets. Code that sticks to the node constructors, `web.style` tokens and the commands of W.8.1 is expected to run there unchanged; `web.raw_html`, `bayata "js"`, `http_routes` and browser-only commands won't be portable.

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
| R7 | A Telugu word must be one people actually type in casual writing (chats, Reddit, comment sections), not one met mainly in newspapers, school grammar, rituals or song lyrics. When people say the English word, use it (R2). Convey the feeling, not the dictionary meaning: `aasajeevi` ("optimist") for unsigned, `Token` for a task handle, `kompa_munigindhi` for a panic |

**Checking R7.** For each candidate word, look at how often and how it appears in Telugu-language online communities (for r7: comments in r/TeluguJournals, r/Ni_Bondha, r/MoviesTelugu, r/hyderabad_telugu and similar). A word that appears mainly in fixed phrases (`gruha pravesham`, `agni pareeksha`, `upama alankaram`) or not at all fails. A word that also means something more common is avoided (`chevi` is mostly "ear", `kaavali` is mostly "want", `velu` is "finger").

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
| `@twaraga` | quickly ("twaraga ra!") | relaxed floating point |
| `@pariksha` | exam (as in "agni pareeksha") | test |
| `migulu` / `Migulochu` | what's left over | remainder `%` |
| `chaalu` | that's enough | `Hasher` finish |
| `byte_mukka` | a piece, in bytes | byte slice of a `padham` |
| `inkokati` | the other one | second operand of operator traits |
| `nidrapo` | go to sleep | task sleep |
| `kalisi` / `pampu` / `Token` / `aagu` | together / send off / token / wait | task scope / spawn / handle / join |
| `Kaluva` | canal | channel |
| `Thaalam` / `theruvu` | lock / open | mutex / lock() |
| `bhadram` | "stay safe" | cryptography module |
| `Rahasyam` | secret | secret value |
| `Raaka` | arrival ("mee raaka") | how a page was reached |
| `Aagna` / `Vinu` | (your royal) command / listen | page command / subscription |
| `maarpu` / `roopam` | change / look | page update / view |
| `kaapala` | guard ("kaapala kaayi") | server function access guard |
| `adugu` | ask | call a server function |
| `Mundhe` | beforehand | pre-rendered page |
| `daari` | way | route |
| `Vaddhu` | no thanks | access refused |
| `edhaina` | anything ("edhaina parledhu") | a Python object |
| `Pythonki` / `Pythonnunchi` | to Python / from Python | conversion traits |
| `ikkade` | right here | scoped Python callbacks |
| `okkate` / `idhenaa` / `nijamaa` | the same one / is it this? / is it true? | `is` / `isinstance` / `bool()` |
| `with_lo` | in a `with` | Python context manager |
| `daani_nunchi` | from that one | `NunchiRavochu` conversion (`From::from`) |

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
| E0216 | code nested deeper than the implementation's limit (§20.6) | "chaala lothuga nest ayyindhi" |
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
| E0603 | move out of a borrow, an index or a closure capture | |
| E0604 | conflict with a shared loan | |
| E0605 | conflict with a mutable loan | |
| E0606 | owner dropped or moved while borrowed | |
| E0607 | `&maare` of a non-mutable place | |
| E0608 | returned reference with no single source | |
| E0609 | use of a maybe-uninitialized value | |
| E0610 | `&maare` argument needs an explicit borrow | "`&maare x` ani raayi" |
| E0611 | a closure changing a variable it captured | "closure lopala bayati `x` ni maarchalem" |
| E0612 | a closure capturing a raw pointer, or a `theesuko` closure capturing a `&maare` reference | |
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
| E1202 | a feature this edition defines that the implementation doesn't support yet (an incomplete implementation reports this instead of accepting the program; the message names the milestone) | |
| **Concurrency and services** (Part IX) | | |
| E1301 | `Pampochu`/`Panchukovochu` implemented by hand outside `naa_risk` | |
| E1302 | a task closure or result that isn't `Pampochu` | "`x` ni vere task ki pampalem" |
| E1303 | `@automatic Raayochu`/`Chadhavochu` on a type with a field that can't be written or read | |
| E1304 | `@lekapothe_kotthadhi` or `@dhaachu` on a field whose type isn't `Kotthadhi` | |
| E1305 | a `form`/`query` type that isn't a flat record | |
| E1306 | a `sql"…"` interpolation of a type with no SQL mapping (§29.3.2) | |
| E1307 | an annotation that isn't allowed in that position, or unknown | |
| **Web** (Annex W) | | |
| E1401 | server-only code reachable from the client build | "idhi server lo matrame pani chesthundhi" |
| E1402 | server function parameter or result that can't be serialized | |
| E1403 | `@server` without a guard or `evarikaina` | "evaru pilavochu? guard pettandi" |
| E1404 | guard function with the wrong signature | |
| E1405 | invalid route pattern, or placeholders that don't match the variant's fields | |
| E1406 | two routes that match the same paths | |
| E1407 | page state that must be serialized but isn't `Raayochu + Chadhavochu` | |
| E1408 | missing translation key in the default locale, or wrong message arguments | |
| E1409 | asset not found | |
| E1410 | empty constant accessible name | |
| E1411 | a page that renders zero or two `web.main` landmarks | |
| E1412 | web package without `app()` | |
| E1413 | a feature that the deploy target can't serve | |
| E1414 | server functions that disagree on the state type | |
| E1415 | `web.role` with a role that needs managed focus | |
| E1416 | a focus style that removes the focus indicator | |
| E1417 | an animation that flashes more than three times a second | |
| E1418 | a production web build without `site_url` | |
| E1501 | a Python import without `features.python = true` | "Python kavalante `ante.toml` lo `python = true` pettandi" |
| E1502 | no Python environment, a version outside `[python] version`, or a module that isn't installed in it | "`numpy` install avvaledhu: `antec python sync` run cheyyandi" |
| E1503 | a name that the module's bindings don't have | "`numpy` lo `zeross` ledhu" |
| E1504 | a value whose type has no Python conversion passed to Python (`sare_thappu`, `pani` outside P.15, `Rahasyam`, iterators, records without the data tree) | |
| E1505 | an owning callback (`python.function_ga`) that holds a borrow | "idhi borrow chesthundhi: `python.ikkade` lo vaadandi" |
| E1506 | a Python import on a target without CPython (the `wasm32` client) | |
| E1507 | a role other than `ni` or a `ga` label in a call to a Python object | |
| E1508 | `nunchi … varaku` in a Python index | "Python index lo `lopu` vaadandi" |
| **Warnings** | | |
| W0001 | unused binding | |
| W0002 | unnecessary `maare` | |
| W0003 | unreachable code | |
| W0005 | unused import | "`x` import chesaru kaani ekkada vaadaledhu" |
| W0100 | deprecated word (§23.5) | "`vegam` peru maarindhi, ippudu `twaraga` ani raayandi" |
| W1401 | text and background colours with contrast below 4.5:1 | |
| W1402 | an interactive target smaller than 24 × 24 px | |
| W1403 | heading levels skip | |
| W1404 | translation missing in a non-default locale | |
| W1406 | a state-changing `http_routes` handler that never checks the request's origin | |
| W1501 | a Python module without type stubs: its names are checked only at run time | |
| W1502 | an argument whose type contradicts the parameter's stub annotation | |

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
# level: core | services | web | torch | python
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
| L006 | Telugu letters outside a literal or comment | E0101 |
| L007 | Backslash at the end of a line (no continuation) | E0201 |
| L008 | String never closed | E0104 |
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
| S016 | Loops of a million iterations that need temporaries | run-pass (no stack growth) |
| S017 | 600 nested parentheses | E0216 |
| S018 | One expression 1,200 operators long | E0216 |
| S019 | `s.enni(1)`: a method call in the constructor form (D10) | E0204 |
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
| T018 | `@twaraga` sum is within the documented tolerance and vectorized (inspect IR) | run-pass |
| T019 | `<` and friends on text (by bytes), `ato_ito` (`kaadhu < avunu`), `aksharam` and tuples | stdout matches |
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
| T037 | `MIN / -1` | T01 |
| T038 | Division, remainder, shifts and float arithmetic | stdout matches |
| T039 | Recursion with no end | T08 |
| T040 | Float display: shortest round-trip form, `.0` on whole numbers however big | stdout matches |
| T041 | `aksharam` predicates follow Unicode | stdout matches |
| T042 | `MIN % -1` with runtime operands | T01 |
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
| O017 | Each loop iteration drops its locals, also on `aapey` | expected stdout |
| O018 | A `theesuko` closure body moves a captured value out | E0603 |
| O019 | `?` in a block nested inside `vellagane` | E0214 |
| O020 | A value passed to a closure by value is dropped when the call ends | expected stdout |
| O021 | A value moved on only one branch, used after the branches join | E0609 |
| O022 | A value moved inside a loop, used again on the next iteration | E0609 |
| O023 | Moving a field out of a value whose type has `sardhu` | E0603 |
| O024 | Using a value as a whole after a partial move | E0601 |
| O025 | A reference outliving the block its referent lives in | E0606 |
| O026 | Mutating a value while a closure that borrows it is still live | E0604 |
| O027 | After a partial move, the remaining fields are still dropped | expected stdout |
| O028 | Assigning to a moved place re-initializes it; nothing is dropped twice | expected stdout |
| O029 | A `&maare` reference passed to two calls in a row (reborrow) | run-pass |
| O030 | A `batti` arm moving one field; the other is dropped with the scrutinee | expected stdout |
| O031 | Moving the contents out of a `pette` with `*b` | run-pass |
| M001 | Non-exhaustive `sare_thappu` match | E0701 |
| M002 | Refutable pattern in a binding | E0703 |
| M003 | Copy binding mode through `&` | run-pass |
| M004 | Variant pattern with the wrong number of fields | E0704 |
| M005 | Nested exhaustive match over tuples and `undochu` | build-pass |
| W004 | Arm covered by earlier arms | W0004 |
| M006 | Arms tried in order: literals, ranges, OR-patterns | stdout matches |
| G001 | Generic `modhatidhi[T]` on `ankam` and `padham` | run-pass |
| G002 | Missing bound used in a generic body | E0801 |
| G003 | Orphan implementation | E0803 |
| G004 | `@automatic Polchochu, Choopinchochu` on a record | run-pass |
| G005 | `CopyKottochu` together with `Sardhukochu` | E0806 |
| G006 | Method pro-drop `balance` → `naa.balance` | run-pass |
| G007 | Closure with a borrowed capture, returned | E0606 |
| G008 | `theesuko` closure returned | run-pass |
| G009 | Implementation missing a required method | E0802 |
| G010 | Same trait implemented twice for a type | E0804 |
| G011 | `@automatic` with a field lacking the trait | E0807 |
| G012 | User trait with a default method, called generically | run-pass |
| G013 | Generic record with a generic methods block and an associated function | expected stdout |
| G014 | Recursive generic enum through `pette` | expected stdout |
| G015 | Bounded generic implementation; user trait with a type parameter | expected stdout |
| G016 | User `Okkokkati` type iterated owned and through adapters | expected stdout |
| G017 | User iterator through `&maare` | expected stdout |
| G018 | `?` converting the error with `NunchiRavochu.daani_nunchi` | expected stdout |
| G019 | `NunchiRavochu` implemented without its method | E0802 |
| G020 | Polymorphic recursion whose types grow forever | E0805 |
| G021 | Copies of a closure share captures, dropped once after the last copy | expected stdout |
| G022 | `theesuko` closure returned and used many times, also by an adapter | expected stdout |
| G023 | Closure changing a captured variable | E0611 |
| G024 | `theesuko` closure capturing a `&maare` reference | E0612 |
| G025 | `@automatic` for every derivable trait | expected stdout |
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
| D004 | Iteration adapters (`jalleda_pattu`, `prathi_dhaaniki`, `poguchey`, `modhati`, `dhaatu`, `kalipi`) | stdout matches |
| D005 | `padham` counts characters, not bytes; parsing and splitting | stdout matches |
| D006 | File errors (missing file, a folder); `file.undhaa` on a folder | stdout matches |
| D007 | `+=` on `padham`, also appending a text to itself and through `&maare` | stdout matches |
| X001 | Program without `python`/`torch` imports | no CPython or LibTorch linkage |
| X002 | `modhalu` returns `thappu(e)` | exit 1, stderr `thappu: …` |
| X003 | `bayata` call outside `naa_risk` | E1001 |
| X004 | `@bayata "ante_kalupu"` export callable from C | integration |
| X005 | Async syntax | E1201 |
| TT001 | Appendix D.6 training step, built without `python` | links LibTorch only (torch level) |
| TT002 | `x[_, -1]` on a 2-D tensor | last column (torch level) |
| TT003 | Shape-mismatched matmul | T05; `torch.choosi` variant returns `thappu` (torch level) |
| PY001 | A Python call that raises | `thappu(PythonThappu)` with `peru` and traceback (python level) |
| PY002 | `ga` label on a positional-only parameter | E0505 (python level) |
| PY003 | Python import without `features.python` | E1501 (python level) |
| PY004 | Misspelled name in a module with stubs (`np.zeross`) | E1503 (python level) |
| PY005 | numpy: `arange`, a mask `a[a > 5]`, `.sum()` | stdout matches (python level, numpy) |
| PY006 | numpy slicing: `lopu` ranges, `_`, `python.slice` with a step, on 2-D arrays | stdout matches (python level, numpy) |
| PY007 | `nunchi … varaku` in a Python index | E1508 (python level) |
| PY008 | `varusa_ga[amsam64]` on float64 and on float32 arrays | the values; then `thappu(TypeError)` (python level, numpy) |
| PY009 | `python.array_ga` moves a `varusa` in, numpy changes it, `varusa_ga` copies it back | stdout matches; clean under a memory checker (python level, numpy) |
| PY010 | `mukka_ga` outside `naa_risk` | E1001 (python level) |
| PY011 | Shape-mismatched `+` on arrays; then `python.choosi.kalupu` | T13 with a shape hint (P.12.4); then `thappu` (python level, numpy) |
| PY012 | pandas: read a CSV from the package, filter with a mask, `groupby().mean()`, `.loc` assignment | stdout matches (python level, pandas) |
| PY013 | scipy `optimize.minimize` with an `ikkade` callback that borrows two vectors | result matches to 1e-6 (python level, scipy) |
| PY014 | A scoped callback kept by Python and called after `ikkade` returned | `thappu` with `RuntimeError` (python level) |
| PY015 | A borrowing closure given to `python.function_ga` | E1505 (python level) |
| PY016 | A trap inside a callback | exit 101 with the trap message (python level) |
| PY017 | matplotlib `savefig` with no display | a PNG file is written (python level, matplotlib) |
| PY018 | OpenCV: `imread` of a missing file is `None`; `cvtColor` of a fixture image gives the expected shape | stdout matches (python level, opencv) |
| PY019 | `python.with_lo` on a `sqlite3` connection whose body gives `thappu` | the row is rolled back (python level) |
| PY020 | Three tasks each `aagu` an `asyncio.sleep(0.2)` | total time below 0.4 s (python level, services) |
| PY021 | An OpenAI client pointed at a local mock server: a call, a streamed response, a 429 told apart with `idhenaa` | stdout matches; no network (python level, openai) |
| PY022 | Hugging Face `pipeline` with a tiny local model fixture; `datasets.map(num_proc=2)` with a function from `python/` | stdout matches; no network (python level, transformers, datasets) |
| PY023 | "Ante" print, Python print, "Ante" print | in that order on stdout (python level) |
| PY024 | Python code calling `sys.exit(3)` | `thappu` with `peru` `SystemExit`; the program goes on (python level) |
| PY025 | Two tasks running numpy work at once | both results correct (python level, numpy, services) |
| PY026 | `python.chadhuvu[Record]` from a `dict` and from a Pydantic model; a dict with an unknown key | the records; then `ChadhuvuThappu.Theliyani` (python level, services) |
| PY027 | A `Rahasyam` passed to a Python call | E1504 (python level, services) |
| PY028 | scikit-learn: k-nearest neighbours on iris with a fixed `random_state` | the accuracy printed (python level, scikit-learn) |
| PY029 | The same built program run with a GIL CPython and a free-threaded one (3.15t) | identical output (python level, numpy) |
| PY030 | `torch.Tensor` from Annex T to Python and back | same storage, no copy (torch and python levels) |

Python-level seeds name the packages they need; the suite pins them in its own `ante.lock`, and none of them uses the network.
| KK001 | Two tasks sum halves of a range (§24.2 example) | stdout matches (services level) |
| KK002 | `pampu` with a closure capturing a raw pointer | E1302 (services level) |
| KK003 | Task captures `&maare x` while the parent reads `x` | borrow error (services level) |
| KK004 | Trap in a task: parent's `aagu` gets `Kompa("T02", …)`, process exits 0 | stdout matches (services level) |
| KK005 | Trap while holding a key, then `theruvu` | T11 (services level) |
| KK006 | `theruvu` twice in one task | T12 (services level) |
| KK007 | Channel of capacity 0: producer and consumer, values in order | stdout matches (services level) |
| KK008 | `edhi_mundhu` with a timer and an empty channel | timer index (services level) |
| KK009 | Cancelling a task in `nidrapo` | `aagu` returns `Cancel` (services level) |
| KK010 | 10 000 tasks incrementing an `Atomic` | 10000 (services level) |
| DT001 | JSON round trip of a record with `undochu`, enum and `jodi` fields | stdout matches (services level) |
| DT002 | Reading JSON with an unknown key | `thappu`, path `.x` (services level) |
| DT003 | JSON with a duplicate key, a NaN, deep nesting | three `thappu`s (services level) |
| DT004 | `@automatic Raayochu` on a record with a `pani` field | E1303 (services level) |
| DT005 | `Rahasyam` in a logged field | E1303 / type error (services level) |
| DT006 | Argon2id hash, then check right and wrong passwords | `Sare`, `Thappu` (services level) |
| DT007 | RFC 3339 parse and print across an offset; `Asia/Kolkata` local time | stdout matches (services level) |
| NW001 | HTTP server and client in one program: GET, POST JSON, 404 | stdout matches (services level) |
| NW002 | Body over the limit | 413 without calling the handler (services level) |
| NW003 | Handler trap | 500, server keeps serving (services level) |
| NW004 | Header value with a newline | `thappu` (services level) |
| NW005 | WebSocket echo | stdout matches (services level) |
| DB001 | SQLite: migrate, insert, `okati`, `anni`, transaction rollback | stdout matches (services level) |
| DB002 | `sql"…"` with an injected-looking string | stored verbatim (services level) |
| DB003 | `sql"…"` interpolating a `pani` | E1306 (services level) |
| WB001 | Annex W.2 counter: SSR HTML, then click twice in `web.pariksha` | "Lekka: 2" (web level) |
| WB002 | `sql` used in a page's `roopam` | E1401 (web level) |
| WB003 | `@server` without a guard | E1403 (web level) |
| WB004 | Two routes `/u/{id}` and `/u/{name}` | E1406 (web level) |
| WB005 | Route variant field count differs from placeholders | E1405 (web level) |
| WB006 | Page state with a `pani` field in `Browser` mode | E1407 (web level) |
| WB007 | Server function called cross-site without `X-Ante-Server` | 403 (web level) |
| WB008 | Text `<script>` in `web.text` | escaped in HTML (web level) |
| WB009 | `web.raw_html` outside `naa_risk` | E1001 (web level) |
| WB010 | Empty constant button label | E1410 (web level) |
| WB011 | Theme with grey text on grey | W1401 (web level) |
| WB012 | Every response's security headers and CSP (W.14) | headers match (web level) |
| WB013 | Settling: `modhalu` asks a server function; the HTML contains its data | HTML matches (web level) |
| WB014 | Hydration keeps the DOM and the state | no DOM replaced (web level) |
| WB015 | `web.pariksha.a11y` on a page with a skipped heading level | one issue (web level) |
| WB016 | Missing translation key | E1408 (web level) |
| WB017 | `deploy = "static"` with a server function | E1413 (web level) |
| WB018 | Live page: click over WebSocket updates the DOM | DOM matches (web level) |
| WB019 | Login rotates the session id | ids differ (web level) |
| WB020 | Deploy skew: unknown server function id | 409, `Version` (web level) |

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

@twaraga
dot_twaraga(a: &mukka[amsam32] ni, b: &mukka[amsam32] tho)
    -> sare_thappu[amsam32, PodavuThappu] ante:
    a.enni() != b.enni() aithe:
        thappu(PodavuThappu.Saripoledhu) icchey
    maare total: amsam32 ante 0.0
    a, b kalipi() lo okko (x, y) ki:
        total += x * y
    sare(total) icchey
```

The strict version gives bit-exact results and can't be vectorized. The `@twaraga` version may reorder the sum so it vectorizes. In production code, use `a ni, b tho lekkalu.dot()`.

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

The recommended architecture: Rust, a hand-written lexer and recursive-descent parser (the D-rules need bounded lookahead and local backtracking, which suits hand-written code better than a generator), an arena AST, then HIR, then typed HIR, then MIR (a CFG of basic blocks with explicit places, moves, loans and drops), then LLVM. (`antec` emits textual LLVM IR and compiles it, together with a small C runtime, with `clang`; this keeps the compiler free of LLVM library dependencies.) Keep a **MIR interpreter** alongside it, so the semantics can be tested before code generation exists, and test the two against each other.

| Milestone | Deliverable | Conformance gate |
|---|---|---|
| M1 | Lexer and layout (§4) | L-series; golden token streams |
| M2 | Parser and AST, `antec emit ast`, `antec fmt` round-trip | S-series syntax cases; fuzzing finds no panics |
| M3 | Modules, name resolution, visibility (§7–8) | E03xx and E09xx |
| M4 | Type checker for monomorphic code: literals, operators, records, enums, patterns (§9, §10, §14) | E04xx, E07xx |
| M5 | Call binding, automatic borrows, pipes (§11–12) | B- and P-series |
| M6 | MIR lowering and MIR interpreter, with traps and drop/`vellagane` order (Part V) | first run-pass and runtime-trap cases |
| M7 | LLVM code generation for core types; a small C runtime for allocation, I/O and traps (parts move to "Ante" once the standard library is written in it) | the same cases, compiled natively on AArch64 and x86-64, except those using `jodi` (native in M10) |
| M8 | Ownership and borrow checker on MIR (§15) | O-series |
| M9 | Generics, traits, monomorphization, `@automatic`, closures (§13, §10.7) | G-series |
| M10 | Standard library (§21), iteration adapters, f-strings | D-series; Appendix D programs |
| M11 | FFI, `naa_risk`, `@C`, exports (§22) | X-series |
| M12 | Tasks and sharing: scheduler, `kalisi`/`pampu`, `Pampochu`/`Panchukovochu` checking, `panchukunna`, `Thaalam`, `Atomic`, `Kaluva`, task traps with unwinding (§24) | KK-series |
| M13 | Data and security: `baitlu`, `encoding`, `Raayochu`/`Chadhavochu` with `@automatic`, `json`/`form`/`query`, `samayam` calendars and zones, `bhadram`, `Rahasyam`, `log`, `config` (§25–27, §29.1–29.2) | DT-series |
| M14 | Network and storage: TCP, TLS, HTTP/1.1 and HTTP/2 server and client, WebSocket, SSE, SMTP, SQLite and PostgreSQL, `sql"…"`, migrations (§28, §29.3) | NW- and DB-series |
| M15 | The `wasm32` client target: code generation, runtime, allocator, the generated loader (Annex W.19) | the run-pass suite in a headless browser |
| M16 | Web pages: nodes, styles and themes, routes, pages, commands and subscriptions, server rendering with settling, hydration, `web.pariksha` (W.5–W.11, W.15–W.18, W.21) | WB001–WB016 |
| M17 | Web server side: server functions, guards, sessions, `web.auth`, security defaults, live mode, offline (W.12–W.14, W.20, W.24, W.25) | WB017–WB020 |
| M18 | `antec web` tooling and deployment targets (W.22–W.23) | integration: deploy each target and run WB001 against it |
| M19 | **The official website**, written in "Ante" and deployed with `antec web`: home page, a browser playground (the reference compiler built for `wasm32`, running fully on the client), the spec and docs, and the learning track below | Lighthouse accessibility 100; every W.14 header; works without a server for `Mundhe` pages |
| M20 | `@twaraga`, `lekkalu.dot` vectorization, performance benchmarks | T017/T018 |
| M21 | Annex T (LibTorch shim generated from `Declarations.yaml`) | TT-series |
| M22 | Annex P: `antec python` and the environment, `edhaina`, bindings from stubs, calls, attributes, indexing and operators, conversions and buffers, errors and learner hints, iteration, context managers, callbacks, the asyncio loop, task and GIL integration, bundling (P.3–P.21) | PY-series |
| M23 | Annex N: native desktop and mobile user interfaces for the same pages (W.27) | to be defined with the annex |
| M24 | `antec repl` and the Jupyter kernel (§23.7) | session cases: bindings across entries, a trap keeps the session, E0601 across entries; a pandas table and a matplotlib figure shown inline |

Core 1.0 is complete at M11, Services 1.0 at M14, Web 1.0 at M18. M20–M22 are independent of the web track and can proceed in parallel with it. M22 needs M12 (tasks, for P.16–P.17) and M13 (the data tree, for P.10.3). M24 needs only M10, and gains inline Python display after M22.

**Learning track** ("Ante" lo DSA). The algorithms of [cp-algorithms](https://cp-algorithms.com) — about 160 articles in algebra, data structures, dynamic programming, graphs, strings, geometry, combinatorics, game theory, linear algebra, numerical methods and sequences — are implemented in "Ante" in a `learn/` folder of the repository, each with tests and an explanation in colloquial Tinglish, and published on the website. cp-algorithms' articles are licensed CC BY-SA 4.0: explanations adapted from them are published under CC BY-SA 4.0 with attribution, in a folder with its own license file, separate from the MIT/Apache-2.0 compiler code. Implementations are written independently from the algorithm descriptions.

| Stage | Needs | Content |
|---|---|---|
| L0 | M10 | computer fundamentals: how numbers live in bits (`ankam8` overflow is T01, `aasajeevi` never goes negative), text as bytes (`byte_mukka`), memory, the stack and the heap (`pette`), ownership and borrowing shown step by step, files and processes |
| L1 | M10 | arithmetic and number theory (binary exponentiation, Euclid, sieve, modular inverse), sorting and searching, prefix sums, two pointers, basic data structures (stack, queue, deque, heap) |
| L2 | M10 | graphs (BFS, DFS, Dijkstra, Bellman-Ford, Floyd–Warshall, MST, topological sort, SCC, bridges), disjoint-set union, segment and Fenwick trees, sparse tables |
| L3 | M10 | dynamic programming, strings (prefix function, Z-function, hashing, suffix array, Aho–Corasick), combinatorics, game theory |
| L4 | M10 (M20 for speed) | geometry, linear algebra, numerical methods, flows and matchings, advanced trees; benchmarks against C++ references |
| L5 | M19 | every algorithm runnable and steppable in the website playground, with visualisations drawn with `web.chitram` |
| L6 | M22, M24 | data basics in notebooks: numpy arrays, pandas tables, plots with matplotlib, images with OpenCV |
| L7 | M22 | classical machine learning with scikit-learn: regression, classification, clustering, evaluation; each model also written from scratch in "Ante" (linear regression, k-nearest neighbours, k-means, decision trees) and checked against scikit-learn |
| L8 | M21, M22 | deep learning: a small automatic-differentiation engine written in "Ante" to show how training works, then the same networks with Annex T (PyTorch), then Hugging Face models |
| L9 | M17, M22 | generative AI applications: calling model APIs (OpenAI, Hugging Face), streaming, retrieval over your own documents, and a full-stack chat web app with Annex W whose server functions call the model |

The tracks are written for Telugu-speaking beginners: every lesson is in colloquial Tinglish, runs in `antec repl` or a notebook, and shows the Python library's own documentation side by side, so learners can keep using it after the course.

---

# Appendix F — Deferred features

These are reserved and rejected with E1201 where syntax exists:

- async/await, generators and `yield` (tasks in Part IX need neither);
- unstructured (detached) tasks;
- web platform features listed in Annex W.26 (pixel canvas, WebGL/WebGPU, media capture, geolocation, push, payments, device APIs, WebRTC, built-in passkeys, streamed HTML, sub-page code splitting);
- Annex N, native desktop and mobile user interfaces;
- trait objects, associated types and specialization;
- lifetime annotations, and references stored in structs;
- macros and reflection;
- Unicode identifiers;
- open-ended slices and spread-holes in pipes;
- function overloading;
- else-less expression forms of `aithe`/`batti`;
- `kitiki` strided views;
- a native tensor type and MLIR codegen;
- the locative receiver form (`account lo 500 ni jama()`);
- constant function calls;
- the Python features listed in Annex P.22.

---

# Appendix G — Change history

Edition 1.0 starts at draft r1, which is the first public revision. Every later change to this spec that affects programs is listed here, newest first, with its revision number and the release that ships it (see `docs/VERSIONING.md`). Before 1.0, new features and clarifications bump the patch version, so several revisions can ship in one release.

| Revision | Release | Change |
|---|---|---|
| r10 | `antec` 0.1.1 | Closures settled while implementing M9 (§10.7): no mutable captures (E0611), no raw-pointer captures and no `&maare` moved into a `theesuko` closure (E0612); `pani` isn't `CopyKottochu` but a place is shared, not moved, with captures dropped after the last handle (§10.7.4); every `pani` is `Pampochu` and `Panchukovochu`. `NunchiRavochu`'s method is `daani_nunchi` (`nunchi` is a keyword, so the method couldn't be written). G007 is E0606; seed cases G013–G025. |
| r9 | `antec` 0.1.1 | Annex P rewritten in detail (P.1–P.22): the Python environment and `antec python`, imports and package-local Python, `edhaina` (now a type keyword, §4.4.2), bindings from stubs, calls, attributes, indexing, operators, conversions, arrays and buffers, errors with learner hints, iteration, context managers, callbacks, async Python, threads and the GIL, output and processes, bundling, notes for numpy, pandas, matplotlib, OpenCV, scipy, scikit-learn, OpenAI, Hugging Face and PyTorch, security; traps T13 and T14; E1501–E1508, W1501–W1502; interactive use (§23.7, `antec repl`, Jupyter); seed cases PY001–PY030; roadmap M22 detailed and M24 added; learning stages L0 and L6–L9. |
| r8 | `antec` 0.1.1 | Ownership details settled while writing the borrow checker (M8): a `&maare` place passed as a phrase is reborrowed, not moved (§11.3); moving a field out of a value whose type implements `Sardhukochu` is E0603 (§15.1.2); partially moved values drop only their remaining fields, and assigning to a moved place drops nothing (§19.3); seed cases O021–O031. |
| r7 | `antec` 0.1.1 | Vocabulary review against how Telugu is actually typed (rule R7, Appendix A.1): `@vegam` → `@twaraga`, `Sheshamochu`/`shesham` → `Migulochu`/`migulu`, `Hasher.mugimpu` → `chaalu`, `byte_bhaagam` → `byte_mukka`, parameters `itharam` → `inkokati` and `sandesham` → `message` (the old names stay accepted with W0100 for one release); in Part IX and Annex W: `raksha` → `bhadram`, `Rasheedu` → `Token`, `Chevi` → `Key`, `Tedhi`/`Velu`/`Pradesham` → `Date`/`Time`/`Zone`, `Pravesham` → `Raaka`, `Aakaaram` → `Shape`, `Rangu` → `Color`, `shaatham` → `percent`, `Madhyama` → `Medium`, `nilva_*` → `save_cheyyi`/`saved_thechu`/`saved_theesey`, `alankaram` → `design_kosam`, guard `kaavali` → `kaapala`, `Anumathi_Ledhu` → `PermissionLedhu`, `sthanam` → `ekkada`, `Raayakam`/`Chadhuvakam` → `Raasevaadu`/`Chadhivevaadu`, `@kotthadhi_unte_dhaatu` → `@lekapothe_kotthadhi`, `web.mata` → `web.maata`. |
| r6 | `antec` 0.1.1 | "Ante" becomes an all-round language: new conformance levels Services and Web (§1.2); Part IX (tasks and sharing, bytes and data formats, time, cryptography, networking and HTTP, email, storage, logs, configuration) and Annex W (web applications); `panchukunna` is now a type; annotations on fields and variants, and module paths in annotation arguments (§5.1); the `sql"…"` literal (§4.5); traps T11 and T12; diagnostics E13xx, E14xx and W14xx; roadmap milestones M12–M23 and the learning track (Appendix E); seed cases KK, DT, NW, DB and WB. |
| r5 | `antec` 0.1.1 | Second audit before M8: moving a captured value out of a `theesuko` closure's body is E0603 (§10.7.2, §15.1.2); the M7 row names `jodi` as native in M10; new seed cases S019, T042, O018–O020, D007. |
| r4 | `antec` 0.1.1 | Hardening after M7: T08/T09 messages may omit the position (§20.3); nesting limits and E0216 (§20.6); the implementation-defined list now includes I/O error text (§20.6); `antec fmt [--check]`, `antec run [path] [-- args]`, `antec build [-o out] [--release]` (§23.1); T019 now tests ordering (the chained comparison case is S014); new seed cases T039–T041, S016–S018, D006, and L006–L008 (already tested, now listed). |
| r3 | `antec` 0.1.1 | Programs run (M6): built-in display format (§13.5); standard library types implement the basic traits (§21.3); runtime seed cases S001–S008, B001–B011, P001–P005, T001–T016, T037–T038, O010–O017, M003, M006, G001–G012, D001–D005, X002. |
| r2 | `antec` 0.1.1 | Clarifications from the type checker (M4): automatic dereference of scalar references in operators (§10.5.1); method lookup skips reference candidates (§13.4.2); numbers implement the arithmetic traits (§13.5); `jalleda_pattu`'s predicate receives `&I` (§21.4.2); Appendix D.7 defines `Model.parameters`; new seed cases T022–T036, P006, O015–O016, M004–M005, W004, G009–G012. |
| r1 | `antec` 0.1.0 | First public draft of Edition 1.0. |

---

# Appendix H — References

- Python language reference: https://docs.python.org/3/reference/
- The Rust Reference (ownership, patterns, traits): https://doc.rust-lang.org/reference/
- Rust RFC 2094, non-lexical lifetimes: https://rust-lang.github.io/rfcs/2094-nll.html
- LLVM Language Reference, fast-math flags: https://llvm.org/docs/LangRef.html#fast-math-flags
- LLVM Language Reference (the textual IR `antec` emits): https://llvm.org/docs/LangRef.html
- `tch-rs` (Rust LibTorch bindings): https://github.com/LaurentMazare/tch-rs
- PyTorch C++ API: https://pytorch.org/cppdocs/
- DLPack: https://dmlc.github.io/dlpack/latest/
- CPython C API, Stable ABI and Limited API: https://docs.python.org/3/c-api/stable.html
- PEP 3118 (buffer protocol), PEP 561 (distributing type information), PEP 703 and PEP 779 (free-threaded CPython), PEP 803 (Stable ABI for free-threaded builds), PEP 734 (multiple interpreters): https://peps.python.org/
- typeshed: https://github.com/python/typeshed
- PyO3 (prior art for embedding CPython from a compiled language): https://pyo3.rs
- pandas 3.0 release notes (copy-on-write, Arrow PyCapsule interface): https://pandas.pydata.org/docs/whatsnew/v3.0.0.html
- Jupyter messaging and display protocol: https://jupyter-client.readthedocs.io/
- Web Content Accessibility Guidelines (WCAG) 2.2: https://www.w3.org/TR/WCAG22/
- WAI-ARIA Authoring Practices Guide: https://www.w3.org/WAI/ARIA/apg/
- OWASP Top 10:2025: https://owasp.org/Top10/2025/
- OWASP Password Storage Cheat Sheet (Argon2id parameters): https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html
- WHATWG URL Standard: https://url.spec.whatwg.org/
- RFC 8259 (JSON), RFC 3339 (timestamps), RFC 3986 (URIs), RFC 4648 (base64), RFC 9106 (Argon2), RFC 9562 (UUIDs), RFC 9325 (TLS recommendations), RFC 6265bis (cookies): https://www.rfc-editor.org/
- WASI 0.3 and the Component Model: https://wasi.dev/releases/wasi-p3
- ICU MessageFormat and CLDR plural rules: https://unicode-org.github.io/icu/userguide/format_parse/messages/ and https://cldr.unicode.org/
- The Elm Architecture (the page model of Annex W.6): https://guide.elm-lang.org/architecture/
- Phoenix LiveView (the model of Annex W.20): https://hexdocs.pm/phoenix_live_view/
- cp-algorithms (the learning track, CC BY-SA 4.0): https://cp-algorithms.com
- IEEE 754-2019, Standard for Floating-Point Arithmetic

---

# Appendix I — Self-hosting readiness

**Goal.** A later `antec` written in "Ante" compiles itself. The Rust `antec` (stage 0) is the first compiler, and this appendix makes sure "Ante" 1.0 can express a compiler, so that self-hosting needs no breaking language changes.

## I.1 What a compiler written in "Ante" needs

| Need | Status in 1.0 | Where |
|---|---|---|
| Read source files and list directories | **added** | `file.chadhuvu`, `file.list_cheyyi`, `file.folderaa`, path helpers (§21.3) |
| Byte-level scanning of source | present, extended | `padham.baitlu()`, `byte_mukka`, `baitlu_nunchi` (§21.2) |
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
| No threads in Core | a single-threaded compiler | Part IX tasks (Services level), for parallel compilation later |
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
