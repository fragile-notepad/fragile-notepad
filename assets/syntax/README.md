# Syntax configuration

[`outline-parsers.xml`](outline-parsers.xml) configures the function list and
navigation; [`folding-hints.xml`](folding-hints.xml) configures folding. Changes
require rebuilding. These rules recognize structure rather than full grammars;
source ranges use UTF-8 byte offsets with exclusive ends.

## Families and bodies

A language selects `use-family`; adapter names are validated metadata.
`delimiter` pairs match outside comments/literals; brace bodies add their pair.
`syntax-token` elements specify a `role` and one-character `value`:

| Roles | Purpose |
| --- | --- |
| `parameters-open`, `parameters-close`, `brackets-open`, `brackets-close` | Signature groups; also need matching `delimiter` elements |
| `generics-open`, `generics-close` | Generic groups |
| `assignment`, `separator`, `statement-end` | Statement segmentation |
| `assignment-reject-before`, `assignment-reject-after` | Exclude compound assignments |
| `type-prefix`, `type-suffix`, `attribute-prefix` | Type literals, pointer/reference suffixes, grouped attributes |

| Body kind | Attributes |
| --- | --- |
| `brace` | Distinct nonempty `open`/`close`; multi-byte delimiters allowed |
| `indent` | `header-end`, `line-continuation`; follows indentation across comments, literals, and grouped continuations |
| `end-keyword` | `end-keyword`, `block-openers`; counts nested blocks |

End-keyword bodies use `conditional-openers` for statement-only conditions,
`loop-openers`/`loop-body-keyword` to count a loop and its `do` once,
`statement-boundaries` to identify statements, and `member-prefixes` to exclude
member names. List attributes are comma-separated.

## Lexical rules

`word-characters` sets extra identifier characters and Unicode XID classes;
`lexical identifier-prefix` shields escaped identifiers from keyword matching.

| Lexical element | Configuration |
| --- | --- |
| `line-comment`, `block-comment` | `open`, block `close`, optional `nested` |
| `string` | `open`, `close`, `escape`; longest opener wins. `requires-closing-on-line` requires an unescaped closer on that line; `single-quote-literals="true"` restricts to character literals |
| `raw-string` | `prefixes`, `open`, `close`, optional `suffix`; closer is `close` + captured delimiter + `suffix`. `repeat` restricts the delimiter to a repeated marker; otherwise it runs to `open`, excluding whitespace and `forbidden-delimiter-characters`. `max-delimiter-length` counts UTF-8 bytes |
| `regex-literal` | `open`, `close`, optional `escape`, paired character-class delimiters; optional `prefix-pattern` matches preceding significant-token context |
| `heredoc` | `prefix-pattern` captures `delimiter` or `delimiter_*`; `indented` or a participating `indent` capture permits whitespace before the closer |
| `line-skip-pattern` | `value` regex consumes directive lines at their first significant position, including continuations |
| `opaque-block` | `prefix-pattern`, balanced `open`/`close`; shields bodies after comment/string masking |

Unterminated literals stay shielded through EOF. Regex context represents closed
control conditions as keyword + `()`, completed blocks as owner + `{}`, and
members with their prefix. Use the bundled language rules as examples.

## Declarations

`container` and `declaration` rules select keywords, names, bodies, terminators,
and callable filters. Language `signature-modifiers` includes preceding modifiers.

| Rule field | Behavior |
| --- | --- |
| `name-pattern` | Anchored regex capturing `name` or `name_*`; skips initial generics. Callable patterns precede parameters; ordinary names are fallback |
| `assignment-arrow` | Assigned block-body functions, such as JavaScript `=>` |
| `compact-constructor-containers` | Constructors without parameters, such as Java `record` |
| `keyword-reject-previous`, `keyword-reject-next` | Exclude adjacent tokens; `@role` references syntax roles |
| `require-statement-start` | Restrict declaration context |
| `require-non-container-previous-kind` | Callable prefixes: `identifier`, `qualified-identifier`, `template-type-tail`, `array-type-tail`, `pointer-type-tail`; constructors remain allowed |
| `qualified-separators` | Qualified-name punctuation |
| `signature-type-braces` | Skip type literals in signatures |
| `signature-brace-prefix-pattern` | Skip brace groups after matching signature prefixes |
| `nextline-body` | Permit bodies after newline terminators |
| `expression-body` | Marker for a body ending on the declaration line |

## Member lists and cache

`members kind="enum-member" within="enum"` lists variants/constants. `separator`
splits top-level entries; `terminator` stops before methods. Nested arguments stay
grouped. `prefix-pattern` skips annotations and balanced arguments; `name-pattern`
captures quoted names as `name`/`name_*`, with ordinary identifiers as fallback.
`line-skip-pattern` skips directive lines and continuations.

Pair `generic-open-pattern` (prefix through opener) with `generic-suffix-pattern`
(balanced group's suffix) and family `generics-open`/`generics-close` tokens to
distinguish generics from comparisons/shifts. Unrecognized members recover at the
next separator.

Empty delimiters, invalid/empty-matching patterns, missing name captures, and
incomplete generic configurations produce diagnostics; invalid lexical rules
exclude the language. `outline-registry.xml` caches plans and rebuilds when
source hashes or structure fail validation.
