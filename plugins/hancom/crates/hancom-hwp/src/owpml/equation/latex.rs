use super::ast::{EqNode, MatrixStyle, PileAlign, SpaceKind};
use super::symbols::{DecoKind, FontStyleKind};

pub(super) fn serialize(node: &EqNode) -> String {
    let mut out = String::new();
    write_node(node, &mut out, false);
    out
}

fn write_node(node: &EqNode, out: &mut String, tabs_are_alignment: bool) {
    match node {
        EqNode::Row(children) => {
            for child in children {
                write_node(child, out, tabs_are_alignment);
            }
        }
        EqNode::Text(text) | EqNode::Number(text) | EqNode::Symbol(text) => {
            write_math_text(text, out)
        }
        EqNode::MathSymbol(symbol) => out.push_str(math_symbol(symbol)),
        EqNode::Function(name) => write_function(name, out),
        EqNode::Fraction { numer, denom } => {
            out.push_str(r"\frac{");
            write_node(numer, out, false);
            out.push_str("}{");
            write_node(denom, out, false);
            out.push('}');
        }
        EqNode::Atop { top, bottom } => {
            out.push_str(r"\begin{array}{c}");
            write_node(top, out, false);
            out.push_str("\\\\ ");
            write_node(bottom, out, false);
            out.push_str(r"\end{array}");
        }
        EqNode::Sqrt { index, body } => {
            out.push_str(r"\sqrt");
            if let Some(index) = index {
                out.push('[');
                write_node(index, out, false);
                out.push(']');
            }
            out.push('{');
            write_node(body, out, false);
            out.push('}');
        }
        EqNode::Superscript { base, sup } => {
            write_script_base(base, out);
            out.push_str("^{");
            write_node(sup, out, false);
            out.push('}');
        }
        EqNode::Subscript { base, sub } => {
            write_script_base(base, out);
            out.push_str("_{");
            write_node(sub, out, false);
            out.push('}');
        }
        EqNode::SubSup { base, sub, sup } => {
            write_script_base(base, out);
            out.push_str("_{");
            write_node(sub, out, false);
            out.push_str("}^{");
            write_node(sup, out, false);
            out.push('}');
        }
        EqNode::LeftSubscript { base, sub } => {
            out.push_str("{}_{");
            write_node(sub, out, false);
            out.push('}');
            write_grouped(base, out);
        }
        EqNode::LeftSuperscript { base, sup } => {
            out.push_str("{}^{");
            write_node(sup, out, false);
            out.push('}');
            write_grouped(base, out);
        }
        EqNode::LeftSubSup { base, sub, sup } => {
            out.push_str("{}_{");
            write_node(sub, out, false);
            out.push_str("}^{");
            write_node(sup, out, false);
            out.push('}');
            write_grouped(base, out);
        }
        EqNode::BigOp { symbol, sub, sup } => {
            out.push_str(big_operator(symbol));
            write_limits(sub.as_deref(), sup.as_deref(), out);
        }
        EqNode::Limit { is_upper, sub } => {
            if *is_upper {
                out.push_str(r"\operatorname{Lim}");
            } else {
                out.push_str(r"\lim");
            }
            if let Some(sub) = sub {
                out.push_str("_{");
                write_node(sub, out, false);
                out.push('}');
            }
        }
        EqNode::Matrix { rows, style } => write_matrix(rows, *style, out),
        EqNode::Cases { rows } => {
            out.push_str(r"\begin{cases}");
            write_rows(rows, out, true);
            out.push_str(r"\end{cases}");
        }
        EqNode::Pile { rows, align } => {
            let alignment = match align {
                PileAlign::Center => 'c',
                PileAlign::Left => 'l',
                PileAlign::Right => 'r',
            };
            out.push_str(r"\begin{array}{");
            out.push(alignment);
            out.push('}');
            write_rows(rows, out, false);
            out.push_str(r"\end{array}");
        }
        EqNode::EqAlign { rows } => {
            out.push_str(r"\begin{aligned}");
            for (index, (left, right)) in rows.iter().enumerate() {
                if index != 0 {
                    out.push_str("\\\\ ");
                }
                write_node(left, out, false);
                out.push('&');
                write_node(right, out, false);
            }
            out.push_str(r"\end{aligned}");
        }
        EqNode::Rel { arrow, over, under } => {
            out.push_str(r"\overset{");
            write_node(over, out, false);
            out.push_str("}{");
            if let Some(under) = under {
                out.push_str(r"\underset{");
                write_node(under, out, false);
                out.push_str("}{");
                write_math_text(arrow, out);
                out.push_str("}}");
            } else {
                write_math_text(arrow, out);
                out.push('}');
            }
        }
        EqNode::Paren { left, right, body } => {
            out.push_str(r"\left");
            out.push_str(delimiter(left));
            write_node(body, out, false);
            out.push_str(r"\right");
            out.push_str(delimiter(right));
        }
        EqNode::Decoration { kind, body } => {
            out.push_str(decoration(*kind));
            out.push('{');
            write_node(body, out, false);
            out.push('}');
        }
        EqNode::FontStyle { style, body } => {
            out.push_str(font_style(*style));
            out.push('{');
            write_node(body, out, false);
            out.push('}');
        }
        EqNode::Color { r, g, b, body } => {
            out.push_str(r"\color{#");
            out.push_str(&format!("{r:02X}{g:02X}{b:02X}"));
            out.push_str("}{");
            write_node(body, out, false);
            out.push('}');
        }
        EqNode::Space(SpaceKind::Normal) => out.push_str(r"\ "),
        EqNode::Space(SpaceKind::Thin) => out.push_str(r"\,"),
        EqNode::Space(SpaceKind::Tab) if tabs_are_alignment => out.push('&'),
        EqNode::Space(SpaceKind::Tab) => out.push_str(r"\quad "),
        EqNode::Newline => out.push_str("\\\\ "),
        EqNode::Quoted(text) => {
            out.push_str(r"\text{");
            write_quoted_text(text, out);
            out.push('}');
        }
        EqNode::Empty => {}
    }
}

fn write_grouped(node: &EqNode, out: &mut String) {
    out.push('{');
    write_node(node, out, false);
    out.push('}');
}

fn write_script_base(node: &EqNode, out: &mut String) {
    if matches!(node, EqNode::MathSymbol(symbol) if matches!(symbol.as_str(), "∫" | "∬" | "∭" | "∮" | "∯" | "∰"))
    {
        // Grouping an integral (`{\int}_a^b`) makes OfficeCLI emit a hidden-limit
        // n-ary node wrapped by a separate script node. Keep the command bare so
        // the bounds become the integral's native OMML sub/sup children.
        write_node(node, out, false);
    } else {
        write_grouped(node, out);
    }
}

fn write_limits(sub: Option<&EqNode>, sup: Option<&EqNode>, out: &mut String) {
    if let Some(sub) = sub {
        out.push_str("_{");
        write_node(sub, out, false);
        out.push('}');
    }
    if let Some(sup) = sup {
        out.push_str("^{");
        write_node(sup, out, false);
        out.push('}');
    }
}

fn write_matrix(rows: &[Vec<EqNode>], style: MatrixStyle, out: &mut String) {
    let environment = match style {
        MatrixStyle::Plain => "matrix",
        MatrixStyle::Paren => "pmatrix",
        MatrixStyle::Bracket => "bmatrix",
        MatrixStyle::Vert => "vmatrix",
    };
    out.push_str(r"\begin{");
    out.push_str(environment);
    out.push('}');
    for (row_index, row) in rows.iter().enumerate() {
        if row_index != 0 {
            out.push_str("\\\\ ");
        }
        for (cell_index, cell) in row.iter().enumerate() {
            if cell_index != 0 {
                out.push('&');
            }
            write_node(cell, out, false);
        }
    }
    out.push_str(r"\end{");
    out.push_str(environment);
    out.push('}');
}

fn write_rows(rows: &[EqNode], out: &mut String, tabs_are_alignment: bool) {
    for (index, row) in rows.iter().enumerate() {
        if index != 0 {
            out.push_str("\\\\ ");
        }
        write_node(row, out, tabs_are_alignment);
    }
}

fn write_function(name: &str, out: &mut String) {
    // OfficeCLI가 지원하는 LaTeX 내장 함수 목록에 기대지 않는다. Hancom의
    // 기본 함수/예약어 전체를 같은 로만체 의미로 보존한다.
    out.push_str(r"\operatorname{");
    write_quoted_text(name, out);
    out.push('}');
}

fn write_math_text(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str(r"\backslash "),
            '{' => out.push_str(r"\{"),
            '}' => out.push_str(r"\}"),
            '#' => out.push_str(r"\#"),
            '$' => out.push_str(r"\$"),
            '%' => out.push_str(r"\%"),
            '&' => out.push_str(r"\&"),
            '_' => out.push_str(r"\_"),
            '^' => out.push_str(r"\hat{}"),
            '~' => out.push_str(r"\sim "),
            _ => out.push(ch),
        }
    }
}

fn write_quoted_text(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str(r"\backslash "),
            '{' => out.push_str(r"\{"),
            '}' => out.push_str(r"\}"),
            '#' => out.push_str(r"\#"),
            '$' => out.push_str(r"\$"),
            '%' => out.push_str(r"\%"),
            '&' => out.push_str(r"\&"),
            '_' => out.push_str(r"\_"),
            '^' => out.push_str(r"\^{}"),
            '~' => out.push_str(r"\~{}"),
            _ => out.push(ch),
        }
    }
}

fn math_symbol(symbol: &str) -> &str {
    match symbol {
        "∫" => r"\int",
        "∬" => r"\iint",
        "∭" => r"\iiint",
        "∮" => r"\oint",
        "∯" => r"\oiint",
        "∰" => r"\oiiint",
        "{" => r"\{",
        "}" => r"\}",
        "\\" => r"\backslash",
        _ => symbol,
    }
}

fn big_operator(symbol: &str) -> &str {
    match symbol {
        "∑" => r"\sum",
        "∏" => r"\prod",
        "∐" => r"\coprod",
        "∪" => r"\bigcup",
        "∩" => r"\bigcap",
        "⊔" => r"\bigsqcup",
        "⊎" => r"\biguplus",
        "⋀" => r"\bigwedge",
        "⋁" => r"\bigvee",
        "⊕" => r"\bigoplus",
        "⊗" => r"\bigotimes",
        "⊙" => r"\bigodot",
        _ => symbol,
    }
}

fn delimiter(value: &str) -> &str {
    match value {
        "" => ".",
        "{" => r"\{",
        "}" => r"\}",
        _ => value,
    }
}

fn decoration(kind: DecoKind) -> &'static str {
    match kind {
        DecoKind::Hat => r"\hat",
        DecoKind::Check => r"\check",
        DecoKind::Tilde => r"\tilde",
        DecoKind::Acute => r"\acute",
        DecoKind::Grave => r"\grave",
        DecoKind::Dot => r"\dot",
        DecoKind::DDot => r"\ddot",
        DecoKind::Bar | DecoKind::Overline => r"\overline",
        DecoKind::Vec => r"\vec",
        DecoKind::Dyad => r"\overleftrightarrow",
        DecoKind::Under => r"\underbrace",
        DecoKind::Arch => r"\overbrace",
        DecoKind::Underline => r"\underline",
        DecoKind::StrikeThrough => r"\cancel",
    }
}

fn font_style(style: FontStyleKind) -> &'static str {
    match style {
        FontStyleKind::Roman => r"\mathrm",
        FontStyleKind::Italic => r"\mathit",
        FontStyleKind::Bold => r"\mathbf",
        FontStyleKind::Blackboard => r"\mathbb",
        FontStyleKind::Calligraphy => r"\mathcal",
        FontStyleKind::Fraktur => r"\mathfrak",
        FontStyleKind::SansSerif => r"\mathsf",
        FontStyleKind::Monospace => r"\mathtt",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_structural_nodes_to_formula_parser_latex() {
        let node = EqNode::Matrix {
            rows: vec![
                vec![EqNode::Text("a".into()), EqNode::Text("b".into())],
                vec![EqNode::Number("1".into()), EqNode::Number("2".into())],
            ],
            style: MatrixStyle::Paren,
        };
        assert_eq!(
            serialize(&node),
            "\\begin{pmatrix}a&b\\\\ 1&2\\end{pmatrix}"
        );
    }
}
