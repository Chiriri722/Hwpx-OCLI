//! 한컴 수식 스크립트 → OfficeCLI가 받는 LaTeX 변환.
//!
//! 토크나이저·AST·재귀 하강 파서는 RHWP의 MIT 코드에서 고정 커밋
//! `496333b27d21ddb9114ba9ae340bcb895870c9a7` 기준으로 가져왔다. 원본 저작권과
//! 라이선스 전문은 `plugins/hancom/NOTICE`에 기록한다. 이 모듈은 그 파서를 곧바로
//! 신뢰하지 않고, 입력·구문·AST·출력 예산과 손실 명령 거부 경계를 덧댄다.

#[allow(clippy::all, dead_code)]
mod ast;
mod latex;
#[allow(clippy::all, dead_code)]
mod parser;
#[allow(clippy::all, dead_code)]
mod symbols;
#[allow(clippy::all, dead_code)]
mod tokenizer;

use std::fmt;

use ast::EqNode;
use symbols::{
    is_big_operator, is_function, is_structure_command, lookup_symbol, DECORATIONS, FONT_STYLES,
};
use tokenizer::{Token, TokenType};

const MAX_SCRIPT_BYTES: usize = 64 * 1024;
const MAX_TOKENS: usize = 8_192;
const MAX_GROUP_DEPTH: usize = 64;
const MAX_PREFIX_DEPTH: usize = 64;
const MAX_AST_NODES: usize = 16_384;
const MAX_AST_DEPTH: usize = 128;
const MAX_MATRIX_CELLS: usize = 4_096;
const MAX_LATEX_BYTES: usize = 256 * 1024;

/// RHWP가 현재 근사 표현으로 낮추거나 명령 이름을 일반 텍스트로 흘리는 구문.
/// 조용한 의미 손실보다 명시적 미지원을 택한다.
const LOSSY_COMMANDS: &[&str] = &[
    "BIGG",
    "BIGDIV",
    "BIGODOT",
    "BIGOPLUS",
    "BIGOTIMES",
    "BIGSQCUP",
    "BIGUPLUS",
    "BIGVEE",
    "BIGWEDGE",
    "BIGMINUS",
    "BIGODIV",
    "BIGOMINUS",
    "BIGSQCAP",
    "COL",
    "LADDER",
    "LCOL",
    "LONGDIV",
    "SCALE",
    "SMALLINT",
    "SMALLOINT",
    "SLADDER",
    "RCOL",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EquationError {
    Invalid(String),
    ResourceLimit(String),
    UnsupportedCommand(String),
}

impl EquationError {
    pub fn is_unsupported(&self) -> bool {
        matches!(self, Self::UnsupportedCommand(_))
    }
}

impl fmt::Display for EquationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(f, "invalid equation script: {message}"),
            Self::ResourceLimit(message) => {
                write!(f, "equation resource limit exceeded: {message}")
            }
            Self::UnsupportedCommand(command) => {
                write!(
                    f,
                    "equation command {command} cannot be converted without loss"
                )
            }
        }
    }
}

impl std::error::Error for EquationError {}

/// 수식 스크립트를 검증하고 OfficeCLI FormulaParser용 정규 LaTeX로 바꾼다.
pub fn to_latex(script: &str) -> Result<String, EquationError> {
    validate_scalar_input(script)?;
    let tokens = tokenizer::tokenize(script);
    validate_tokens(&tokens)?;

    let mut parser = parser::EqParser::new(tokens);
    let ast = parser.parse_checked().map_err(|position| {
        EquationError::Invalid(format!(
            "parser stopped before character {position} instead of consuming the input"
        ))
    })?;
    validate_ast(&ast)?;

    let latex = latex::serialize(&ast);
    if latex.trim().is_empty() {
        return Err(EquationError::Invalid(
            "parser produced an empty expression".to_string(),
        ));
    }
    if latex.len() > MAX_LATEX_BYTES {
        return Err(EquationError::ResourceLimit(format!(
            "LaTeX output is {} bytes (maximum {MAX_LATEX_BYTES})",
            latex.len()
        )));
    }
    Ok(latex)
}

fn validate_scalar_input(script: &str) -> Result<(), EquationError> {
    if script.trim().is_empty() {
        return Err(EquationError::Invalid("script is empty".to_string()));
    }
    if script.len() > MAX_SCRIPT_BYTES {
        return Err(EquationError::ResourceLimit(format!(
            "script is {} bytes (maximum {MAX_SCRIPT_BYTES})",
            script.len()
        )));
    }
    if let Some(ch) = script
        .chars()
        .find(|ch| ch.is_control() && !matches!(ch, '\t' | '\n' | '\r'))
    {
        return Err(EquationError::Invalid(format!(
            "disallowed control character U+{:04X}",
            u32::from(ch)
        )));
    }
    if let Some(ch) = script
        .chars()
        .find(|ch| is_private_use(*ch) && *ch != '\u{E04D}')
    {
        return Err(EquationError::UnsupportedCommand(format!(
            "unknown private-use symbol U+{:04X}",
            u32::from(ch)
        )));
    }

    let mut quoted = false;
    let mut brace_depth = 0usize;
    for ch in script.chars() {
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        match ch {
            '{' => {
                brace_depth = brace_depth.saturating_add(1);
                if brace_depth > MAX_GROUP_DEPTH {
                    return Err(EquationError::ResourceLimit(format!(
                        "group nesting exceeds {MAX_GROUP_DEPTH}"
                    )));
                }
            }
            '}' if brace_depth == 0 => {
                return Err(EquationError::Invalid(
                    "closing brace has no matching opening brace".to_string(),
                ));
            }
            '}' => brace_depth -= 1,
            _ => {}
        }
    }
    if quoted {
        return Err(EquationError::Invalid(
            "quoted text is not terminated".to_string(),
        ));
    }
    if brace_depth != 0 {
        return Err(EquationError::Invalid(
            "opening brace is not terminated".to_string(),
        ));
    }
    Ok(())
}

fn validate_tokens(tokens: &[Token]) -> Result<(), EquationError> {
    let token_count = tokens
        .iter()
        .filter(|token| token.ty != TokenType::Eof)
        .count();
    if token_count > MAX_TOKENS {
        return Err(EquationError::ResourceLimit(format!(
            "script has {token_count} tokens (maximum {MAX_TOKENS})"
        )));
    }

    let mut left_right_depth = 0usize;
    let mut prefix_depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        if token.ty == TokenType::Command && is_recursive_prefix_command(&token.value) {
            prefix_depth = prefix_depth.saturating_add(1);
            if prefix_depth > MAX_PREFIX_DEPTH {
                return Err(EquationError::ResourceLimit(format!(
                    "prefix nesting exceeds {MAX_PREFIX_DEPTH}"
                )));
            }
        } else {
            prefix_depth = 0;
        }
        if token.ty == TokenType::Text && token.value.chars().any(|ch| ch.is_ascii()) {
            return Err(EquationError::Invalid(format!(
                "unrecognized ASCII token {:?} at character {}",
                token.value, token.pos
            )));
        }
        if token.ty != TokenType::Command {
            continue;
        }
        let command = token.value.to_ascii_uppercase();
        if matches!(command.as_str(), "FROM" | "TO") {
            if !is_valid_integral_bound_marker(tokens, index) {
                return Err(EquationError::UnsupportedCommand(command));
            }
            continue;
        }
        if is_lossy_command(&command) {
            return Err(EquationError::UnsupportedCommand(command));
        }
        if !is_known_command(&token.value) {
            return Err(EquationError::UnsupportedCommand(format!(
                "unknown word {:?} at character {}",
                token.value, token.pos
            )));
        }
        if command == "COLOR" {
            validate_color_argument(tokens, index)?;
        }
        match command.as_str() {
            "LEFT" => {
                left_right_depth = left_right_depth.saturating_add(1);
                if left_right_depth > MAX_GROUP_DEPTH {
                    return Err(EquationError::ResourceLimit(format!(
                        "LEFT/RIGHT nesting exceeds {MAX_GROUP_DEPTH}"
                    )));
                }
            }
            "RIGHT" if left_right_depth == 0 => {
                return Err(EquationError::Invalid(
                    "RIGHT has no matching LEFT".to_string(),
                ));
            }
            "RIGHT" => left_right_depth -= 1,
            _ => {}
        }
    }
    if left_right_depth != 0 {
        return Err(EquationError::Invalid(
            "LEFT has no matching RIGHT".to_string(),
        ));
    }
    Ok(())
}

fn is_recursive_prefix_command(command: &str) -> bool {
    let upper = command.to_ascii_uppercase();
    let lower = command.to_ascii_lowercase();
    DECORATIONS.contains_key(command)
        || DECORATIONS.contains_key(lower.as_str())
        || FONT_STYLES.contains_key(command)
        || FONT_STYLES.contains_key(lower.as_str())
        || matches!(
            upper.as_str(),
            "SQRT"
                | "ROOT"
                | "FRAC"
                | "DFRAC"
                | "TFRAC"
                | "TEXT"
                | "OPERATORNAME"
                | "PHANTOM"
                | "VPHANTOM"
                | "HPHANTOM"
                | "OVERSET"
                | "UNDERSET"
                | "STACKREL"
                | "REL"
                | "BUILDREL"
                | "BINOM"
                | "CHOOSE"
                | "SUP"
                | "SUB"
                | "LSUP"
                | "LSUB"
                | "COLOR"
        )
}

fn is_valid_integral_bound_marker(tokens: &[Token], index: usize) -> bool {
    let command = tokens[index].value.to_ascii_uppercase();
    let upper_operand_exists = || bound_operand_end(tokens, index + 1).is_some();

    if command == "FROM" {
        return index
            .checked_sub(1)
            .and_then(|previous| tokens.get(previous))
            .is_some_and(is_integral_token)
            && upper_operand_exists();
    }

    if command != "TO" || !upper_operand_exists() {
        return false;
    }
    if index
        .checked_sub(1)
        .and_then(|previous| tokens.get(previous))
        .is_some_and(is_integral_token)
    {
        return true;
    }

    (1..index).rev().any(|from_index| {
        tokens[from_index].ty == TokenType::Command
            && tokens[from_index].value.eq_ignore_ascii_case("FROM")
            && tokens.get(from_index - 1).is_some_and(is_integral_token)
            && bound_operand_end(tokens, from_index + 1) == Some(index)
    })
}

fn is_integral_token(token: &Token) -> bool {
    token.ty == TokenType::Command
        && matches!(
            token.value.to_ascii_uppercase().as_str(),
            "INT"
                | "INTEGRAL"
                | "SMALLINT"
                | "DINT"
                | "TINT"
                | "OINT"
                | "SMALLOINT"
                | "ODINT"
                | "OTINT"
        )
}

fn bound_operand_end(tokens: &[Token], start: usize) -> Option<usize> {
    let first = tokens.get(start)?;
    if first.ty == TokenType::Eof || first.ty == TokenType::RBrace {
        return None;
    }
    if first.ty != TokenType::LBrace {
        return Some(start + 1);
    }

    let mut depth = 0usize;
    for (offset, token) in tokens[start..].iter().enumerate() {
        match token.ty {
            TokenType::LBrace => depth = depth.saturating_add(1),
            TokenType::RBrace => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(start + offset + 1);
                }
            }
            TokenType::Eof => return None,
            _ => {}
        }
    }
    None
}

fn is_lossy_command(command: &str) -> bool {
    LOSSY_COMMANDS.contains(&command)
        || command.strip_prefix("SCALE").is_some_and(|percent| {
            !percent.is_empty() && percent.chars().all(|ch| ch.is_ascii_digit())
        })
}

fn is_known_command(command: &str) -> bool {
    let upper = command.to_ascii_uppercase();
    let lower = command.to_ascii_lowercase();
    upper == "OF"
        || upper == "LIM"
        || upper == "EQALIGN"
        || is_structure_command(command)
        || is_structure_command(&upper)
        || is_big_operator(command)
        || is_big_operator(&upper)
        || is_function(command)
        || is_function(&lower)
        || lookup_symbol(command).is_some()
        || DECORATIONS.contains_key(command)
        || DECORATIONS.contains_key(lower.as_str())
        || FONT_STYLES.contains_key(command)
        || FONT_STYLES.contains_key(lower.as_str())
        // 수식 r1.3은 공백으로 나뉜 영숫자 항을 변수로 정의한다. 9자 이상은
        // 따옴표로 묶어야 하므로 여기서는 최대 8자까지만 일반 항으로 받는다.
        || (command.len() <= 8
            && command.chars().all(|ch| ch.is_ascii_alphanumeric()))
}

fn validate_color_argument(tokens: &[Token], command_index: usize) -> Result<(), EquationError> {
    let expected = [
        TokenType::LBrace,
        TokenType::Number,
        TokenType::Symbol,
        TokenType::Number,
        TokenType::Symbol,
        TokenType::Number,
        TokenType::RBrace,
    ];
    let start = command_index + 1;
    if start + expected.len() > tokens.len() {
        return Err(EquationError::Invalid(
            "COLOR requires {R,G,B}{body}".to_string(),
        ));
    }
    let slice = &tokens[start..start + expected.len()];
    if slice.iter().zip(expected).any(|(token, ty)| token.ty != ty)
        || slice[2].value != ","
        || slice[4].value != ","
    {
        return Err(EquationError::Invalid(
            "COLOR requires three comma-separated decimal channels".to_string(),
        ));
    }
    for token in [&slice[1], &slice[3], &slice[5]] {
        if token.value.parse::<u8>().is_err() {
            return Err(EquationError::Invalid(format!(
                "COLOR channel {:?} is outside 0..255",
                token.value
            )));
        }
    }
    Ok(())
}

fn is_private_use(ch: char) -> bool {
    matches!(
        u32::from(ch),
        0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD
    )
}

fn validate_ast(root: &EqNode) -> Result<(), EquationError> {
    let mut nodes = 0usize;
    walk_ast(root, 1, &mut nodes)?;
    if is_empty_node(root) {
        return Err(EquationError::Invalid(
            "expression contains no renderable node".to_string(),
        ));
    }
    Ok(())
}

fn walk_ast(node: &EqNode, depth: usize, nodes: &mut usize) -> Result<(), EquationError> {
    if depth > MAX_AST_DEPTH {
        return Err(EquationError::ResourceLimit(format!(
            "AST nesting exceeds {MAX_AST_DEPTH}"
        )));
    }
    *nodes = nodes.saturating_add(1);
    if *nodes > MAX_AST_NODES {
        return Err(EquationError::ResourceLimit(format!(
            "AST has more than {MAX_AST_NODES} nodes"
        )));
    }

    match node {
        EqNode::Row(children) => {
            for child in children {
                walk_ast(child, depth + 1, nodes)?;
            }
        }
        EqNode::Fraction { numer, denom } => {
            require_operand(numer, "fraction numerator")?;
            require_operand(denom, "fraction denominator")?;
            walk_ast(numer, depth + 1, nodes)?;
            walk_ast(denom, depth + 1, nodes)?;
        }
        EqNode::Atop { top, bottom } => {
            require_operand(top, "ATOP top")?;
            require_operand(bottom, "ATOP bottom")?;
            walk_ast(top, depth + 1, nodes)?;
            walk_ast(bottom, depth + 1, nodes)?;
        }
        EqNode::Sqrt { index, body } => {
            require_operand(body, "square-root body")?;
            if let Some(index) = index {
                require_operand(index, "square-root index")?;
                walk_ast(index, depth + 1, nodes)?;
            }
            walk_ast(body, depth + 1, nodes)?;
        }
        EqNode::Superscript { base, sup } => {
            require_operand(base, "superscript base")?;
            require_operand(sup, "superscript value")?;
            walk_ast(base, depth + 1, nodes)?;
            walk_ast(sup, depth + 1, nodes)?;
        }
        EqNode::Subscript { base, sub } => {
            require_operand(base, "subscript base")?;
            require_operand(sub, "subscript value")?;
            walk_ast(base, depth + 1, nodes)?;
            walk_ast(sub, depth + 1, nodes)?;
        }
        EqNode::SubSup { base, sub, sup } => {
            require_operand(base, "script base")?;
            require_operand(sub, "subscript value")?;
            require_operand(sup, "superscript value")?;
            walk_ast(base, depth + 1, nodes)?;
            walk_ast(sub, depth + 1, nodes)?;
            walk_ast(sup, depth + 1, nodes)?;
        }
        EqNode::LeftSubscript { base, sub } => {
            require_operand(base, "left-subscript base")?;
            require_operand(sub, "left-subscript value")?;
            walk_ast(base, depth + 1, nodes)?;
            walk_ast(sub, depth + 1, nodes)?;
        }
        EqNode::LeftSuperscript { base, sup } => {
            require_operand(base, "left-superscript base")?;
            require_operand(sup, "left-superscript value")?;
            walk_ast(base, depth + 1, nodes)?;
            walk_ast(sup, depth + 1, nodes)?;
        }
        EqNode::LeftSubSup { base, sub, sup } => {
            require_operand(base, "left-script base")?;
            require_operand(sub, "left-subscript value")?;
            require_operand(sup, "left-superscript value")?;
            walk_ast(base, depth + 1, nodes)?;
            walk_ast(sub, depth + 1, nodes)?;
            walk_ast(sup, depth + 1, nodes)?;
        }
        EqNode::BigOp { sub, sup, .. } => {
            if let Some(sub) = sub {
                require_operand(sub, "operator lower limit")?;
                walk_ast(sub, depth + 1, nodes)?;
            }
            if let Some(sup) = sup {
                require_operand(sup, "operator upper limit")?;
                walk_ast(sup, depth + 1, nodes)?;
            }
        }
        EqNode::Limit { sub, .. } => {
            if let Some(sub) = sub {
                require_operand(sub, "limit condition")?;
                walk_ast(sub, depth + 1, nodes)?;
            }
        }
        EqNode::Matrix { rows, .. } => {
            let Some(width) = rows.first().map(Vec::len).filter(|width| *width > 0) else {
                return Err(EquationError::Invalid(
                    "matrix contains no cells".to_string(),
                ));
            };
            if rows.iter().any(|row| row.len() != width) {
                return Err(EquationError::Invalid(
                    "matrix rows have different column counts".to_string(),
                ));
            }
            let cells = rows
                .len()
                .checked_mul(width)
                .ok_or_else(|| EquationError::ResourceLimit("matrix size overflow".to_string()))?;
            if cells > MAX_MATRIX_CELLS {
                return Err(EquationError::ResourceLimit(format!(
                    "matrix has {cells} cells (maximum {MAX_MATRIX_CELLS})"
                )));
            }
            for row in rows {
                for cell in row {
                    walk_ast(cell, depth + 1, nodes)?;
                }
            }
        }
        EqNode::Cases { rows } | EqNode::Pile { rows, .. } => {
            if rows.is_empty() {
                return Err(EquationError::Invalid(
                    "multi-row structure contains no rows".to_string(),
                ));
            }
            for row in rows {
                walk_ast(row, depth + 1, nodes)?;
            }
        }
        EqNode::EqAlign { rows } => {
            if rows.is_empty() {
                return Err(EquationError::Invalid(
                    "EQALIGN contains no rows".to_string(),
                ));
            }
            for (left, right) in rows {
                walk_ast(left, depth + 1, nodes)?;
                walk_ast(right, depth + 1, nodes)?;
            }
        }
        EqNode::Rel { over, under, .. } => {
            require_operand(over, "relation upper label")?;
            walk_ast(over, depth + 1, nodes)?;
            if let Some(under) = under {
                require_operand(under, "relation lower label")?;
                walk_ast(under, depth + 1, nodes)?;
            }
        }
        EqNode::Paren { body, .. }
        | EqNode::Decoration { body, .. }
        | EqNode::FontStyle { body, .. }
        | EqNode::Color { body, .. } => {
            require_operand(body, "command body")?;
            walk_ast(body, depth + 1, nodes)?;
        }
        EqNode::Text(_)
        | EqNode::Number(_)
        | EqNode::Symbol(_)
        | EqNode::MathSymbol(_)
        | EqNode::Function(_)
        | EqNode::Space(_)
        | EqNode::Newline
        | EqNode::Quoted(_)
        | EqNode::Empty => {}
    }
    Ok(())
}

fn require_operand(node: &EqNode, name: &str) -> Result<(), EquationError> {
    if is_empty_node(node) {
        return Err(EquationError::Invalid(format!("{name} is missing")));
    }
    Ok(())
}

fn is_empty_node(node: &EqNode) -> bool {
    match node {
        EqNode::Empty => true,
        EqNode::Row(children) => children.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_the_hwpxlib_reference_equation() {
        assert_eq!(
            to_latex(r#"{"123"} over {123 sqrt {3466}} sum _{34} ^{12}"#).unwrap(),
            r#"\frac{\text{123}}{123\sqrt{3466}}\sum_{34}^{12}"#
        );
    }

    #[test]
    fn rejects_unbalanced_or_excessively_nested_input() {
        assert!(matches!(
            to_latex("sqrt {1"),
            Err(EquationError::Invalid(_))
        ));
        let deep = format!("{}1{}", "{".repeat(65), "}".repeat(65));
        assert!(matches!(
            to_latex(&deep),
            Err(EquationError::ResourceLimit(_))
        ));
    }

    #[test]
    fn rejects_known_lossy_commands() {
        for command in LOSSY_COMMANDS {
            assert!(matches!(
                to_latex(&format!("{command} {{1}}")),
                Err(EquationError::UnsupportedCommand(found)) if found == *command
            ));
        }
    }

    #[test]
    fn rejects_ambiguous_words_unknown_pua_and_invalid_rgb() {
        assert!(matches!(
            to_latex("futurecommand {x}"),
            Err(EquationError::UnsupportedCommand(message)) if message.contains("unknown word")
        ));
        assert!(matches!(
            to_latex("x + \u{E123}"),
            Err(EquationError::UnsupportedCommand(message)) if message.contains("private-use")
        ));
        assert!(matches!(
            to_latex("COLOR {256,0,0} {x}"),
            Err(EquationError::Invalid(message)) if message.contains("0..255")
        ));
    }

    #[test]
    fn accepts_official_short_terms_functions_and_reserved_words() {
        assert_eq!(
            to_latex(
                "E=mc^2 + SINx + cosecy + arcsinhz + asina + acosb + atanc + Expd + logw + detM + deg e + if p for q and r"
            )
            .unwrap(),
            concat!(
                r"E={mc}^{2}+\operatorname{sin}x+\operatorname{cosec}y",
                r"+\operatorname{arcsinh}z+\operatorname{asin}a",
                r"+\operatorname{acos}b+\operatorname{atan}c",
                r"+\operatorname{Exp}d+\operatorname{log}w+\operatorname{det}M",
                r"+\operatorname{deg}e+\operatorname{if}p",
                r"\operatorname{for}q\operatorname{and}r"
            )
        );
    }

    #[test]
    fn converts_official_operator_aliases_without_command_leakage() {
        let cases = [
            ("DIVIDE", "÷"),
            ("SUPSET", "⊃"),
            ("SQCUP", "⊔"),
            ("SQCAP", "⊓"),
            ("UPLUS", "⊎"),
            ("OPLUS", "⊕"),
            ("OMINUS", "⊖"),
            ("OTIMES", "⊗"),
            ("ODOT", "⊙"),
            ("OSLASH", "⊘"),
            ("SMALLSUM", "Σ"),
            ("SMALLPROD", "∏"),
            ("SMCOPROD", "∐"),
            ("BENZENE", "⌬"),
            ("LBRACE", r"\{"),
            ("RBRACE", r"\}"),
            ("RSLANT", r"\backslash"),
        ];

        for (script, expected) in cases {
            assert_eq!(to_latex(script).unwrap(), expected, "{script}");
        }
    }

    #[test]
    fn converts_official_integral_bounds_and_left_scripts() {
        assert_eq!(to_latex("int FROM 0 TO 3").unwrap(), r"\int_{0}^{3}");
        assert_eq!(to_latex("x LSUB i").unwrap(), r"{}_{i}{x}");
        assert_eq!(to_latex("x LSUP j").unwrap(), r"{}^{j}{x}");
        assert_eq!(to_latex("x LSUB i LSUP j").unwrap(), r"{}_{i}^{j}{x}");
    }

    #[test]
    fn rejects_contextual_equation_commands_outside_their_grammar() {
        for script in ["FROM 0", "TO 3", "int FROM", "int FROM 0 TO"] {
            assert!(to_latex(script).is_err(), "{script:?} must fail closed");
        }
        assert!(matches!(
            to_latex("SCALE70 x"),
            Err(EquationError::UnsupportedCommand(command)) if command == "SCALE70"
        ));
    }

    #[test]
    fn enforces_script_token_and_group_boundaries() {
        let exact_script = "1".repeat(MAX_SCRIPT_BYTES);
        assert!(to_latex(&exact_script).is_ok());
        assert!(matches!(
            to_latex(&format!("{exact_script}1")),
            Err(EquationError::ResourceLimit(_))
        ));

        let exact_tokens = "1 ".repeat(MAX_TOKENS);
        assert!(to_latex(&exact_tokens).is_ok());
        assert!(matches!(
            to_latex(&format!("{exact_tokens}1")),
            Err(EquationError::ResourceLimit(message)) if message.contains("tokens")
        ));

        let exact_groups = format!(
            "{}1{}",
            "{".repeat(MAX_GROUP_DEPTH),
            "}".repeat(MAX_GROUP_DEPTH)
        );
        assert!(to_latex(&exact_groups).is_ok());
        let excessive_groups = format!(
            "{}1{}",
            "{".repeat(MAX_GROUP_DEPTH + 1),
            "}".repeat(MAX_GROUP_DEPTH + 1)
        );
        assert!(matches!(
            to_latex(&excessive_groups),
            Err(EquationError::ResourceLimit(message)) if message.contains("group nesting")
        ));

        let exact_prefixes = format!("{}x", "hat ".repeat(MAX_PREFIX_DEPTH));
        assert!(to_latex(&exact_prefixes).is_ok());
        let excessive_prefixes = format!("{}x", "hat ".repeat(MAX_PREFIX_DEPTH + 1));
        assert!(matches!(
            to_latex(&excessive_prefixes),
            Err(EquationError::ResourceLimit(message)) if message.contains("prefix nesting")
        ));
    }

    #[test]
    fn enforces_ast_matrix_and_output_boundaries() {
        let exact_ast = EqNode::Row(
            (0..MAX_AST_NODES - 1)
                .map(|_| EqNode::Number("1".to_string()))
                .collect(),
        );
        assert!(validate_ast(&exact_ast).is_ok());
        let excessive_ast = EqNode::Row(
            (0..MAX_AST_NODES)
                .map(|_| EqNode::Number("1".to_string()))
                .collect(),
        );
        assert!(matches!(
            validate_ast(&excessive_ast),
            Err(EquationError::ResourceLimit(message)) if message.contains("AST")
        ));

        let exact_matrix = EqNode::Matrix {
            rows: vec![vec![EqNode::Empty; MAX_MATRIX_CELLS]],
            style: ast::MatrixStyle::Plain,
        };
        assert!(validate_ast(&exact_matrix).is_ok());
        let excessive_matrix = EqNode::Matrix {
            rows: vec![vec![EqNode::Empty; MAX_MATRIX_CELLS + 1]],
            style: ast::MatrixStyle::Plain,
        };
        assert!(matches!(
            validate_ast(&excessive_matrix),
            Err(EquationError::ResourceLimit(message)) if message.contains("matrix")
        ));

        // Quoted content is one input token but escaping can expand the LaTeX output.
        let exact_output_script = format!("\"{}\\___\"", "^".repeat(MAX_SCRIPT_BYTES - 6));
        let exact_output = to_latex(&exact_output_script).unwrap();
        assert_eq!(exact_output.len(), MAX_LATEX_BYTES);

        let excessive_output_script = format!("\"{}\\__\"", "^".repeat(MAX_SCRIPT_BYTES - 5));
        assert!(matches!(
            to_latex(&excessive_output_script),
            Err(EquationError::ResourceLimit(message)) if message.contains("LaTeX output")
        ));
    }

    #[test]
    fn rejects_orphan_operators_and_ragged_matrices() {
        for script in ["OVER 2", "1 OVER", "^2", "x_"] {
            assert!(
                matches!(to_latex(script), Err(EquationError::Invalid(_))),
                "{script:?}"
            );
        }
        assert!(matches!(
            to_latex("MATRIX {a & b # c}"),
            Err(EquationError::Invalid(message)) if message.contains("different column counts")
        ));
    }

    #[test]
    fn converts_the_closed_structural_support_set() {
        let cases = [
            ("a atop b", r"\begin{array}{c}a\\ b\end{array}"),
            (
                "MATRIX {a & b # c & d}",
                r"\begin{matrix}a&b\\ c&d\end{matrix}",
            ),
            (
                "PMATRIX {a & b # c & d}",
                r"\begin{pmatrix}a&b\\ c&d\end{pmatrix}",
            ),
            (
                "BMATRIX {a & b # c & d}",
                r"\begin{bmatrix}a&b\\ c&d\end{bmatrix}",
            ),
            (
                "DMATRIX {a & b # c & d}",
                r"\begin{vmatrix}a&b\\ c&d\end{vmatrix}",
            ),
            (
                "CASES {x & x>0 # -x & x<0}",
                r"\begin{cases}x&x>0\\ -x&x<0\end{cases}",
            ),
            ("LPILE {a # b}", r"\begin{array}{l}a\\ b\end{array}"),
            (
                "EQALIGN {a & =b # c & =d}",
                r"\begin{aligned}a&=b\\ c&=d\end{aligned}",
            ),
            ("REL rarrow {x} {y}", r"\overset{x}{\underset{y}{→}}"),
            (
                "BINOM {n} {r}",
                r"\left(\begin{array}{c}n\\ r\end{array}\right)",
            ),
            (
                "n CHOOSE r",
                r"\left(\begin{array}{c}n\\ r\end{array}\right)",
            ),
            ("x SUB i SUP 2", r"{x}_{i}^{2}"),
            ("LEFT | x RIGHT | ^2", r"{\left|x\right|}^{2}"),
            ("bold x it y rm z", r"\mathbf{x}\mathit{y}\mathrm{z}"),
            ("hat x UNDERLINE y", r"\hat{x}\underline{y}"),
        ];

        for (script, expected) in cases {
            assert_eq!(to_latex(script).unwrap(), expected, "{script}");
        }
    }
}
