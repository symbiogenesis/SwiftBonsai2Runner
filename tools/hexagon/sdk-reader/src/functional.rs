//! Conservative lexical metadata. Nothing here evaluates SDK code or qualifies ABI rights.

use crate::{hex, limit};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) fn ascii_json(value: &Value) -> Result<Vec<u8>> {
    use std::fmt::Write as _;
    let text = serde_json::to_string(value)?;
    let mut ascii = String::new();
    for character in text.chars() {
        if character.is_ascii() {
            ascii.push(character);
        } else {
            let mut units = [0_u16; 2];
            for unit in character.encode_utf16(&mut units) {
                write!(ascii, "\\u{unit:04x}")?;
            }
        }
    }
    Ok(ascii.into_bytes())
}

fn token_hash(tokens: &[String]) -> String {
    // Token strings have already been checked; serialization of strings cannot fail.
    let bytes = ascii_json(&json!(tokens)).unwrap_or_default();
    hex(&Sha256::digest(bytes))
}

fn decode_wide(bytes: &[u8], width: usize, big: bool) -> Option<String> {
    if !bytes.len().is_multiple_of(width) {
        return None;
    }
    if width == 2 {
        let units = bytes.chunks_exact(2).map(|c| {
            if big {
                u16::from_be_bytes([c[0], c[1]])
            } else {
                u16::from_le_bytes([c[0], c[1]])
            }
        });
        char::decode_utf16(units)
            .collect::<std::result::Result<String, _>>()
            .ok()
    } else {
        bytes
            .chunks_exact(4)
            .map(|c| {
                char::from_u32(if big {
                    u32::from_be_bytes([c[0], c[1], c[2], c[3]])
                } else {
                    u32::from_le_bytes([c[0], c[1], c[2], c[3]])
                })
            })
            .collect()
    }
}

pub(crate) fn decode(bytes: &[u8]) -> (Option<String>, Value) {
    for (bom, name, width, big) in [
        (b"\xff\xfe\0\0".as_slice(), "utf-32", 4, false),
        (b"\0\0\xfe\xff".as_slice(), "utf-32", 4, true),
        (b"\xff\xfe".as_slice(), "utf-16", 2, false),
        (b"\xfe\xff".as_slice(), "utf-16", 2, true),
        (b"\xef\xbb\xbf".as_slice(), "utf-8-sig", 1, false),
    ] {
        if let Some(rest) = bytes.strip_prefix(bom) {
            let text = if width == 1 {
                std::str::from_utf8(rest).ok().map(str::to_owned)
            } else {
                decode_wide(rest, width, big)
            };
            let valid = text.is_some();
            return (
                text,
                json!({"encoding":name,"basis":if valid {"explicit-BOM"} else {"invalid-BOM-declared-text"},"uncertain":!valid}),
            );
        }
    }
    let mut attempts = Vec::new();
    match std::str::from_utf8(bytes) {
        Ok(text) if !text.contains('\0') => {
            return (
                Some(text.to_owned()),
                json!({"encoding":"utf-8","basis":"strict-decode-no-BOM","uncertain":false}),
            );
        }
        Ok(_) => attempts.push("UTF8-decoded-with-NULs".to_owned()),
        Err(_) => attempts.push("strict-UTF8-failed".to_owned()),
    }
    if bytes.len() >= 4 && bytes.len().is_multiple_of(2) {
        for parity in [1, 0] {
            if bytes
                .iter()
                .skip(parity)
                .step_by(2)
                .filter(|b| **b == 0)
                .count()
                * 10
                > bytes.len() / 2 * 6
            {
                let name = if parity == 1 {
                    "utf-16-le"
                } else {
                    "utf-16-be"
                };
                if let Some(text) = decode_wide(bytes, 2, parity == 0) {
                    return (
                        Some(text),
                        json!({"encoding":name,"basis":"NUL-pattern-Windows-text-fallback-no-BOM","uncertain":true,"priorAttempts":attempts}),
                    );
                }
                attempts.push(format!("{name}-failed"));
            }
        }
    }
    const CP1252: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    let text = bytes
        .iter()
        .map(|b| {
            if (128..160).contains(b) {
                CP1252[usize::from(*b - 128)]
            } else {
                char::from(*b)
            }
        })
        .collect();
    (
        Some(text),
        json!({"encoding":"cp1252","basis":"Windows-1252-fallback-no-BOM","uncertain":true,"priorAttempts":attempts}),
    )
}

pub(crate) struct Lex {
    pub(crate) tokens: Vec<String>,
    lines: Vec<usize>,
    directives: Vec<Value>,
    splices: usize,
}

struct Lexer {
    lex: Lex,
    line: usize,
    limit: usize,
    directive: Option<(usize, Vec<String>)>,
}
impl Lexer {
    fn emit(&mut self, token: &str) -> Result<()> {
        if self.lex.tokens.len() >= self.limit || token.encode_utf16().count() > 4096 {
            return Err("functional-token-count-or-length-bound".into());
        }
        self.lex.tokens.push(token.to_owned());
        self.lex.lines.push(self.line);
        if !["<PP-BEGIN>", "<PP-END>"].contains(&token) {
            if let Some((_, tokens)) = &mut self.directive {
                tokens.push(token.to_owned());
            }
        }
        Ok(())
    }
    fn end_directive(&mut self) -> Result<()> {
        if self.directive.is_some() {
            self.emit("<PP-END>")?;
            let (line, tokens) = self.directive.take().ok_or("missing directive")?;
            self.lex
                .directives
                .push(json!({"line":line,"tokens":tokens}));
        }
        Ok(())
    }
}
fn identifier(text: &str) -> bool {
    let mut bytes = text.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

pub(crate) fn lex(text: &str, limit: usize) -> Result<Lex> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let splices = normalized.matches("\\\n").count();
    let text = normalized.replace("\\\n", "");
    let mut state = Lexer {
        lex: Lex {
            tokens: Vec::new(),
            lines: Vec::new(),
            directives: Vec::new(),
            splices,
        },
        line: 1,
        limit,
        directive: None,
    };
    let (mut cursor, mut previous_end, mut line_start) = (0, usize::MAX, true);
    while cursor < text.len() {
        let tail = &text[cursor..];
        let ch = tail.chars().next().ok_or("missing source character")?;
        if ch.is_whitespace() {
            if ch == '\n' {
                state.end_directive()?;
                state.line += 1;
                line_start = true;
            }
            cursor += ch.len_utf8();
            continue;
        }
        if tail.starts_with("//") {
            cursor += tail.find('\n').unwrap_or(tail.len());
            continue;
        }
        if tail.starts_with("/*") {
            let end = tail.find("*/").ok_or("unterminated-block-comment")? + 2;
            let lines = tail[..end].matches('\n').count();
            if lines != 0 {
                state.end_directive()?;
                state.line += lines;
                line_start = true;
            }
            cursor += end;
            continue;
        }
        if ch == '#' && line_start {
            state.directive = Some((state.line, Vec::new()));
            state.emit("<PP-BEGIN>")?;
        }
        if let Some((_, directive)) = &state.directive {
            if directive.len() == 2 && directive[0] == "#" && directive[1] == "include" && ch == '<'
            {
                let end = tail.find('>').ok_or("unterminated-include-header-name")? + 1;
                if tail[..end].contains('\n') {
                    return Err("unterminated-include-header-name".into());
                }
                state.emit(&tail[..end])?;
                cursor += end;
                previous_end = cursor;
                line_start = false;
                continue;
            }
            if directive.len() == 3 && directive[0] == "#" && directive[1] == "define" && ch == '('
            {
                state.emit(if previous_end == cursor {
                    "<PP-MACRO-FUNCTIONLIKE>"
                } else {
                    "<PP-MACRO-OBJECTLIKE-PAREN>"
                })?;
            }
        }
        let literal = [
            "u8\"", "u8'", "u\"", "u'", "U\"", "U'", "L\"", "L'", "\"", "'",
        ]
        .iter()
        .find(|prefix| tail.starts_with(**prefix));
        let length = if let Some(prefix) = literal {
            let quote = prefix.as_bytes()[prefix.len() - 1];
            let mut end = prefix.len();
            let mut closed = false;
            while end < tail.len() {
                if tail.as_bytes()[end] == b'\\' {
                    end += 2;
                } else if tail.as_bytes()[end] == quote {
                    end += 1;
                    closed = true;
                    break;
                } else if tail.as_bytes()[end] == b'\n' {
                    return Err("newline-in-unclosed-literal".into());
                } else {
                    end += 1;
                }
            }
            if !closed {
                return Err("unterminated-quoted-literal".into());
            }
            end
        } else if ch.is_ascii_alphabetic() || ch == '_' {
            tail.bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
                .count()
        } else if ch.is_ascii_digit()
            || (ch == '.' && tail.as_bytes().get(1).is_some_and(u8::is_ascii_digit))
        {
            let mut end = 1;
            while end < tail.len() {
                let b = tail.as_bytes()[end];
                if b.is_ascii_alphanumeric()
                    || b"_.".contains(&b)
                    || (b"+-".contains(&b) && b"eEpP".contains(&tail.as_bytes()[end - 1]))
                {
                    end += 1;
                } else {
                    break;
                }
            }
            end
        } else if let Some(operator) = [
            ">>=", "<<=", "...", "->", "++", "--", "==", "!=", "<=", ">=", "&&", "||", "+=", "-=",
            "*=", "/=", "%=", "&=", "|=", "^=", "<<", ">>", "::", "##",
        ]
        .iter()
        .find(|op| tail.starts_with(**op))
        {
            operator.len()
        } else if "{}[]();,:?~!+-*/%&|^<>=.#".contains(ch) {
            1
        } else {
            return Err("unsupported-functional-source-character".into());
        };
        state.emit(&tail[..length])?;
        cursor += length;
        previous_end = cursor;
        line_start = false;
    }
    state.end_directive()?;
    Ok(state.lex)
}

fn declarations(lex: &Lex, limits: &Value) -> Result<Value> {
    let mut code = Vec::new();
    let mut pp = false;
    for (token, line) in lex.tokens.iter().zip(&lex.lines) {
        if token == "<PP-BEGIN>" {
            pp = true;
        } else if token == "<PP-END>" {
            pp = false;
        } else if !pp {
            code.push((token.clone(), *line));
        }
    }
    let (mut cursor, mut braces, mut parens, mut brackets) = (0, 0_i32, 0_i32, 0_i32);
    let mut current: Vec<String> = Vec::new();
    let mut start = 0;
    let mut found = Vec::new();
    while cursor < code.len() {
        let (token, line) = &code[cursor];
        if current.is_empty()
            && token == "extern"
            && code.get(cursor + 1).is_some_and(|v| v.0 == "\"C\"")
            && code.get(cursor + 2).is_some_and(|v| v.0 == "{")
        {
            cursor += 3;
            continue;
        }
        if current.is_empty() && token == "}" {
            cursor += 1;
            continue;
        }
        if current.is_empty() {
            start = *line;
        }
        current.push(token.clone());
        match token.as_str() {
            "(" => parens += 1,
            ")" => parens -= 1,
            "[" => brackets += 1,
            "]" => brackets -= 1,
            "{" => braces += 1,
            "}" => braces -= 1,
            _ => {}
        }
        let pending = |reason, found: &Vec<Value>| json!({"status":"pending-declaration-review","reason":reason,"declarations":found,"unprocessedTailTokens":code.len()-cursor,"typeCheckingExecuted":false});
        if braces.min(parens).min(brackets) < 0 {
            return Ok(pending("unbalanced-declaration-delimiter", &found));
        }
        if token == ";" && braces == 0 && parens == 0 && brackets == 0 {
            let names: BTreeSet<_> = current
                .iter()
                .filter(|token| {
                    identifier(token)
                        && ["dspqueue", "fastrpc", "remote", "callback", "rpcmem"]
                            .iter()
                            .any(|part| token.to_lowercase().contains(part))
                })
                .cloned()
                .collect();
            let typedef = current.iter().any(|token| token == "typedef");
            let enumeration = current.iter().any(|token| token == "enum");
            if !names.is_empty() || typedef || enumeration {
                if current.len() > usize::try_from(limit(limits, "declarationTokens")?)?
                    || found.len() >= usize::try_from(limit(limits, "functionalDeclarations")?)?
                {
                    return Ok(pending("functional-declaration-output-bound", &found));
                }
                let callbacks: Vec<_> = current
                    .windows(4)
                    .filter(|w| w[0] == "(" && w[1] == "*" && w[3] == ")")
                    .map(|w| w[2].clone())
                    .collect();
                let methods: BTreeSet<_> = current
                    .windows(2)
                    .filter(|w| {
                        identifier(&w[0])
                            && w[1] == "("
                            && ![
                                "if",
                                "while",
                                "for",
                                "switch",
                                "sizeof",
                                "defined",
                                "_Alignof",
                                "__attribute__",
                                "__declspec",
                            ]
                            .contains(&w[0].as_str())
                    })
                    .map(|w| w[0].clone())
                    .collect();
                let alias = if typedef {
                    callbacks.last().cloned().or_else(|| {
                        current
                            .iter()
                            .rev()
                            .skip(1)
                            .find(|token| identifier(token))
                            .cloned()
                    })
                } else {
                    None
                };
                found.push(json!({"startLine":start,"identifierCandidates":names,"typedefPresent":typedef,"enumPresent":enumeration,"typedefAliasNameCandidate":alias,"methodNameCandidates":methods,"callbackTypedefNameCandidates":if typedef {callbacks} else {Vec::new()},"normalizedTokenSha256":token_hash(&current),"tokens":current,"conditionalPreprocessorNotEvaluated":true}));
            }
            current.clear();
        }
        cursor += 1;
    }
    Ok(
        json!({"status":"lexical-candidates-only-not-an-ABI-parser","declarations":found,"unclosedOrNonDeclarationTailTokens":current.len(),"typeCheckingExecuted":false}),
    )
}

pub(crate) fn metadata(bytes: &[u8], limits: &Value) -> Result<Value> {
    let (text, observation) = decode(bytes);
    let mut result = json!({"encodingObservation":observation,"macrosEvaluated":false,"ABIEquivalenceQualified":false,"normalization":"C-phase2-line-splices; comment-aware-C-IDL-lexing; canonical-JSON-ASCII-token-array; explicit-preprocessor-boundaries-and-macro-adjacency; literal-include-header-names"});
    let Some(text) = text.filter(|text| !text.contains('\0')) else {
        result["status"] = json!("pending-functional-text-decoding");
        return Ok(result);
    };
    let lex = match lex(&text, usize::try_from(limit(limits, "functionalTokens")?)?) {
        Ok(lex) => lex,
        Err(error) => {
            result["status"] = json!("pending-functional-token-review");
            result["reason"] = json!(error.to_string());
            return Ok(result);
        }
    };
    result["status"] = json!("normalized-functional-tokens-observed-only");
    result["normalizedTokenSha256"] = json!(token_hash(&lex.tokens));
    result["tokenCount"] = json!(lex.tokens.len());
    result["lineSplicesRemoved"] = json!(lex.splices);
    if lex.directives.len() > usize::try_from(limit(limits, "preprocessorDirectives")?)? {
        result["preprocessorStatus"] = json!("pending-preprocessor-metadata-bound");
    } else {
        for (field, command) in [
            ("includeDirectives", "include"),
            ("macroDefinitions", "define"),
        ] {
            result[field] = json!(
                lex.directives
                    .iter()
                    .filter(|d| d["tokens"][0] == "#" && d["tokens"][1] == command)
                    .collect::<Vec<_>>()
            );
        }
        result["preprocessorDirectives"] = json!(lex.directives);
    }
    result["declarationCandidates"] = declarations(&lex, limits)?;
    Ok(result)
}

pub(crate) fn compare(sdk: &[u8], public: &[u8], limits: &Value) -> Result<Value> {
    let left = metadata(sdk, limits)?;
    let right = metadata(public, limits)?;
    let complete =
        left.get("normalizedTokenSha256").is_some() && right.get("normalizedTokenSha256").is_some();
    let mut indexes: [BTreeMap<String, BTreeSet<String>>; 2] = [BTreeMap::new(), BTreeMap::new()];
    for (index, meta) in [&left, &right].into_iter().enumerate() {
        if let Some(declarations) = meta["declarationCandidates"]["declarations"].as_array() {
            for declaration in declarations {
                let mut names = Vec::new();
                for field in [
                    "identifierCandidates",
                    "methodNameCandidates",
                    "callbackTypedefNameCandidates",
                ] {
                    if let Some(values) = declaration[field].as_array() {
                        names.extend(values.iter().filter_map(Value::as_str));
                    }
                }
                if let Some(alias) = declaration["typedefAliasNameCandidate"].as_str() {
                    names.push(alias);
                }
                for name in names {
                    indexes[index].entry(name.to_owned()).or_default().insert(
                        declaration["normalizedTokenSha256"]
                            .as_str()
                            .ok_or("declaration digest missing")?
                            .to_owned(),
                    );
                }
            }
        }
    }
    let shared: Vec<_> = indexes[0]
        .keys()
        .filter(|name| indexes[1].contains_key(*name))
        .collect();
    let named: Vec<_> = shared.iter().take(usize::try_from(limit(limits, "functionalDeclarations")?)?).map(|name| json!({"identifier":name,"sdkCandidateTokenHashes":indexes[0][*name],"publicCandidateTokenHashes":indexes[1][*name],"fullLexicalCandidateSetsEqual":indexes[0][*name] == indexes[1][*name]})).collect();
    Ok(
        json!({"status":if complete {"lexical-comparison-only"} else {"pending-functional-token-review"},"sdk":left,"public":right,"rawBytesEqual":sdk == public,"normalizedTokenStreamsEqual":if complete {json!(left["normalizedTokenSha256"] == right["normalizedTokenSha256"])} else {Value::Null},"sameIdentifierCandidateComparisons":named,"sameIdentifierCandidateComparisonsTruncated":named.len() != shared.len(),"matchingNamesOrTokensProveABIEquivalence":false,"WindowsRuntimeCompatibilityQualified":false,"callbackLifetimeQualified":false,"cacheAndBufferOwnershipQualified":false,"installedDriverAbiQualified":false,"deviceInvocationAuthorized":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{inventory, recipe, strict_json};
    fn limits() -> Value {
        json!({"limits":{"functionalTokens":10000,"functionalDeclarations":64,"declarationTokens":256,"preprocessorDirectives":64,"recipeFunctions":16,"recipeStatements":256,"recipeStatementTokens":128,"recipeTokenCharacters":512}})
    }
    fn tokens(text: &str) -> Vec<String> {
        lex(text, 10000).expect("valid source fixture").tokens
    }
    #[test]
    fn test_comments_do_not_consume_literal_or_include_contents() {
        let actual = tokens(
            "#include <api // header.h>\nconst char *remote_url = \"https://x/*literal*/\"; /* tail */\n",
        );
        assert!(actual.iter().any(|t| t == "<api // header.h>"));
        assert!(actual.iter().any(|t| t == "\"https://x/*literal*/\""));
        assert!(!actual.iter().any(|t| t == "tail"));
        assert_eq!(
            tokens("int remote_x /* comment */ ;"),
            ["int", "remote_x", ";"]
        );
    }
    #[test]
    fn test_macro_function_adjacency_and_directive_ends_remain_distinct() {
        let function = tokens("#define FN(x) x\n");
        let object = tokens("#define FN (x) x\n");
        assert!(function.iter().any(|t| t == "<PP-MACRO-FUNCTIONLIKE>"));
        assert!(object.iter().any(|t| t == "<PP-MACRO-OBJECTLIKE-PAREN>"));
        assert_eq!(object, tokens("#define FN/**/(x) x\n"));
        assert_ne!(function, object);
        assert_ne!(tokens("#define X 1\nY"), tokens("#define X 1 Y\n"));
    }
    #[test]
    fn test_line_splices_precede_comment_recognition() {
        assert_eq!(
            tokens("#define FN(x) ((x) \\\n+ 1)\n"),
            tokens("#define FN(x) ((x) + 1)\n")
        );
        assert!(tokens("// comment\\\nint remote_x;\n").is_empty());
    }
    #[test]
    fn test_literal_prefix_adjacency_is_not_whitespace() {
        assert_eq!(tokens("L\"x\""), ["L\"x\""]);
        assert_ne!(tokens("L\"x\""), tokens("L \"x\""));
    }
    #[test]
    fn test_typedef_callback_enum_and_method_candidates_preserve_tokens() -> Result<()> {
        let meta = metadata(b"typedef void (*dspqueue_callback)(int fd, void *ctx);\nenum dspqueue_flags { DSPQUEUE_BUFFER_REF=4, DSPQUEUE_BUFFER_DEREF=8 };\nint dspqueue_read(int fd, unsigned timeout);\n", &limits())?;
        let declarations = meta["declarationCandidates"]["declarations"]
            .as_array()
            .ok_or("declaration fixture")?;
        assert_eq!(declarations.len(), 3);
        assert_eq!(
            declarations[0]["callbackTypedefNameCandidates"],
            json!(["dspqueue_callback"])
        );
        assert_eq!(declarations[1]["enumPresent"], true);
        let comparison = compare(
            b"int remote_call(int arg);",
            b"short remote_call(int arg);",
            &limits(),
        )?;
        assert_eq!(comparison["normalizedTokenStreamsEqual"], false);
        assert_eq!(
            comparison["matchingNamesOrTokensProveABIEquivalence"],
            false
        );
        assert_eq!(comparison["callbackLifetimeQualified"], false);
        Ok(())
    }
    #[test]
    fn test_unknown_source_syntax_preserves_pending_state() -> Result<()> {
        let meta = metadata(b"int @remote_value;", &limits())?;
        assert_eq!(meta["status"], "pending-functional-token-review");
        assert!(meta.get("normalizedTokenSha256").is_none());
        let comparison = compare(b"int @remote_value;", b"int remote_value;", &limits())?;
        assert!(comparison["normalizedTokenStreamsEqual"].is_null());
        assert_eq!(comparison["deviceInvocationAuthorized"], false);
        Ok(())
    }
    #[test]
    fn test_known_boms_and_uncertain_windows_text_fallback() {
        for (bom, width) in [(b"\xff\xfe".as_slice(), 2), (b"\xff\xfe\0\0".as_slice(), 4)] {
            let mut bytes = bom.to_vec();
            for unit in "QAIC terms".encode_utf16() {
                if width == 2 {
                    bytes.extend_from_slice(&unit.to_le_bytes());
                } else {
                    bytes.extend_from_slice(&u32::from(unit).to_le_bytes());
                }
            }
            let (text, observation) = decode(&bytes);
            assert_eq!(text.as_deref(), Some("QAIC terms"));
            assert_eq!(observation["basis"], "explicit-BOM");
            assert_eq!(observation["uncertain"], false);
        }
        let review = inventory::legal(b"\x93QAIC\x94 product license agreement", 25);
        assert_eq!(review["encodingObservation"]["encoding"], "cp1252");
        assert_eq!(review["encodingObservation"]["uncertain"], true);
        assert_eq!(review["termOccurrences"]["qaic-name"]["count"], 1);
        assert_eq!(review["keywordMatchesEstablishGrant"], false);
        assert_eq!(review["sdkToolTermsQualified"], false);
        assert!(
            review["necessaryReviewExcerpt"]["text"]
                .as_str()
                .is_some_and(|text| text.split_whitespace().count() <= 25)
        );
        assert!(decode(b"\xff\xfe\0").0.is_none());
    }
    #[test]
    fn test_cmake_recipe_keeps_balanced_tokens_and_pending_authority() -> Result<()> {
        let text = "# ignored (comment)\nset(HEXAGON_SDK_ROOT \"C:/sdk with spaces\")\nfunction(build_idl project)\nhelper(\"${HEXAGON_SDK_ROOT}/qaic\" [=[literal #(content)]=])\nmessage(\"Nonfunctional prose\")\nendfunction()\nfunction(helper tool)\nadd_custom_command(COMMAND \"${tool}\" --standard-types COMMENT \"omit comment prose\")\nendfunction()\n";
        let statements = recipe::statements(text)?;
        assert_eq!(statements.len(), 8);
        assert_eq!(statements[0]["tokens"][1], "\"C:/sdk with spaces\"");
        assert_eq!(statements[2]["tokens"][1], "[=[literal #(content)]=]");
        let recipes = BTreeMap::from([(
            "sdk/build/cmake/hexagon_fun.cmake".to_owned(),
            text.as_bytes().to_vec(),
        )]);
        let review = recipe::metadata(&recipes, &limits())?;
        assert_eq!(review["functions"].as_array().map(Vec::len), Some(2));
        assert_eq!(review["variableExpansionExecuted"], false);
        assert_eq!(review["generatorCommandAuthorized"], false);
        assert_eq!(
            review["functions"][0]["statements"][2]["nonfunctionalMessageTextOmitted"],
            true
        );
        assert!(recipe::statements("function(build_idl \"unterminated)").is_err());
        Ok(())
    }
    #[test]
    fn test_sdk_pe_and_notice_metadata_never_grant_rights() {
        let mut data = [0_u8; 96];
        data[..2].copy_from_slice(b"MZ");
        data[60..64].copy_from_slice(&64_u32.to_le_bytes());
        data[64..70].copy_from_slice(b"PE\0\0\x64\xaa");
        assert_eq!(inventory::pe(&data)["machineHex"], "aa64");
        let notice = inventory::notice(&data);
        assert_eq!(
            notice["encodingObservation"]["basis"],
            "validated-PE-header; text-decoding-not-attempted"
        );
        assert_eq!(notice["licenseRightsInterpretationReviewed"], false);
        data[60..64].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(inventory::pe(&data)["validPeHeader"], false);
    }
    #[test]
    fn test_metadata_bounds_and_duplicate_json_are_fail_closed() -> Result<()> {
        assert!(lex("int remote_x;", 2).is_err());
        assert!(lex("/* unterminated", 32).is_err());
        assert!(lex("\"unterminated", 32).is_err());
        assert!(strict_json::parse(br#"{"limits":{},"limits":{}}"#).is_err());
        assert!(strict_json::parse(br#"{"a":{"x":1,"x":2}}"#).is_err());
        assert!(strict_json::parse(br#"{"a":1} false"#).is_err());
        let mut bound = limits();
        bound["limits"]["functionalDeclarations"] = json!(1);
        let meta = metadata(b"typedef int remote_a; typedef int remote_b;", &bound)?;
        assert_eq!(
            meta["declarationCandidates"]["status"],
            "pending-declaration-review"
        );
        assert_eq!(meta["ABIEquivalenceQualified"], false);
        Ok(())
    }
}
