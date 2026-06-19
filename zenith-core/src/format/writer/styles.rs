//! Style-block writing: the `styles { … }` block and each `style` entry,
//! including recognized and unknown style properties.

use crate::ast::StyleBlock;

use super::{escape_kdl_string, fmt_property_value, indent};

pub(super) fn write_style_block(block: &StyleBlock, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("styles {\n");

    for style in &block.styles {
        let has_body = !style.properties.is_empty() || !style.unknown_props.is_empty();
        indent(out, depth + 1);
        out.push_str("style id=\"");
        out.push_str(&style.id);
        out.push('"');

        if has_body {
            out.push_str(" {\n");

            // Recognized properties in BTreeMap (sorted) key order — deterministic.
            for (key, value) in &style.properties {
                indent(out, depth + 2);
                out.push_str(key);
                out.push(' ');
                out.push_str(&fmt_property_value(value));
                out.push('\n');
            }

            // Unknown properties in sorted key order.
            for (key, prop) in &style.unknown_props {
                indent(out, depth + 2);
                out.push_str(key);
                out.push_str(" \"");
                out.push_str(&escape_kdl_string(&prop.raw));
                out.push_str("\"\n");
            }

            indent(out, depth + 1);
            out.push_str("}\n");
        } else {
            out.push('\n');
        }
    }

    indent(out, depth);
    out.push_str("}\n");
}
