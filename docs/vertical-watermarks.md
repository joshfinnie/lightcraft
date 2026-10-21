# Vertical text watermarks

Set `watermark.vertical` to `true` in export options, or choose Vertical in the export dialog. Text runs down each column; a newline starts the next column to its left. The size, nine anchors, horizontal and vertical insets, opacity, colour and shadow use the same controls as horizontal watermarks. Existing presets without `vertical` remain horizontal.

## Character cells

A vertical cell contains one extended grapheme cluster, not one Unicode scalar. For example, `é` and `e` followed by U+0301 COMBINING ACUTE ACCENT occupy the same space and produce the same image. The source string is not normalized or rewritten. CRLF and LF each start one new column.

The renderer shapes the complete cluster, retaining all resulting glyphs and their offsets, even when a base and its combining mark remain separate glyphs. It rejects a shaped cluster containing a missing glyph: the first character then fills the cell (Inter's missing-glyph box when Inter lacks it) rather than allocating cells to its marks or variation selectors.

Text is drawn in Inter, LightCraft's only font, upright in every cell; em and en dashes are turned to run along the column. Characters Inter has no glyph for, CJK among them, draw as its missing-glyph box. This is not a vertical paragraph composer: it has no vertical glyph forms, rotated Latin runs, tate-chu-yoko, ruby, kinsoku, automatic column wrapping or mojikumi spacing.

## References and scope

- [Unicode UAX #29, §3 Grapheme Cluster Boundaries](https://www.unicode.org/reports/tr29/#Grapheme_Cluster_Boundaries) specifically identifies vertical-text segmentation as a use for grapheme clusters and states that boundaries must not split combining sequences. This is the basis for the cells.
- [Lightroom Classic's public watermark workflow](https://helpx.adobe.com/lightroom-classic/help/using-watermark-editor.html) documents text/graphic watermarks, opacity, size, inset and nine anchors. It does not document a vertical-text mode. LightCraft's vertical cells are an extension, not a claim to match an undocumented Lightroom mode.

Regression coverage in `crates/engine/src/export.rs` compares actual coverage pixels for composed and decomposed text, checks multi-glyph mark placement and the right-to-left columns, and draws characters Inter lacks.
