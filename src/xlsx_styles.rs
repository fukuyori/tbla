//! Hand-parse the subset of an .xlsx file's styling that tbla cares about:
//! font color, fill color (background), and horizontal alignment. Calamine
//! only exposes values and formulas, so we open the same .xlsx as a ZIP
//! archive, walk `xl/styles.xml`, and then walk each sheet's XML to map
//! `(col, row) -> resolved style`.
//!
//! Limitations:
//! - Theme colors and indexed colors resolve to None (we don't load
//!   `xl/theme/theme1.xml`); only direct `rgb="AARRGGBB"` colors come
//!   through.
//! - Italic/underline/borders/etc. are intentionally skipped.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::cell::{Alignment, RgbColor};

#[derive(Clone, Default, Debug)]
pub struct CellStyle {
    pub font_color: Option<RgbColor>,
    pub bg_color: Option<RgbColor>,
    pub alignment: Alignment,
    pub bold: bool,
}

/// Per-sheet map of `(col, row) -> style` parsed from the .xlsx file.
pub struct WorkbookStyles {
    /// Workbook-order list of sheet names from xl/workbook.xml.
    pub sheet_names: Vec<String>,
    /// Same order as `sheet_names`. Each map is keyed by 0-based (col, row).
    pub sheet_styles: Vec<HashMap<(usize, usize), CellStyle>>,
    /// Same order as `sheet_names`. Conditional formatting rules parsed
    /// from each sheet's `<conditionalFormatting>` elements (with dxf
    /// colors resolved from `xl/styles.xml`).
    pub sheet_conditionals: Vec<Vec<crate::sheet::ConditionalFormat>>,
}

pub fn read_workbook_styles(path: &str) -> Result<WorkbookStyles, String> {
    let f = File::open(path).map_err(|e| format!("xlsx open: {}", e))?;
    let mut archive = zip::ZipArchive::new(f).map_err(|e| format!("xlsx zip: {}", e))?;

    let styles_xml = read_xml(&mut archive, "xl/styles.xml").unwrap_or_default();
    let cell_xfs = parse_styles(&styles_xml);
    let dxfs = parse_dxfs(&styles_xml);

    // workbook.xml → sheet order + relationship IDs
    let workbook_xml = read_xml(&mut archive, "xl/workbook.xml")
        .ok_or_else(|| "missing xl/workbook.xml".to_string())?;
    let rels_xml = read_xml(&mut archive, "xl/_rels/workbook.xml.rels").unwrap_or_default();
    let sheets = parse_workbook(&workbook_xml, &rels_xml);

    let mut sheet_styles = Vec::with_capacity(sheets.len());
    let mut sheet_conditionals = Vec::with_capacity(sheets.len());
    let mut sheet_names = Vec::with_capacity(sheets.len());
    for (name, target) in sheets {
        sheet_names.push(name);
        let path = format!("xl/{}", target.trim_start_matches('/').trim_start_matches("xl/"));
        let sheet_xml = read_xml(&mut archive, &path).unwrap_or_default();
        let cell_style_refs = parse_sheet_cell_styles(&sheet_xml);
        let resolved: HashMap<(usize, usize), CellStyle> = cell_style_refs
            .into_iter()
            .filter_map(|((c, r), xf_idx)| cell_xfs.get(xf_idx).cloned().map(|st| ((c, r), st)))
            .filter(|(_, st)| st.font_color.is_some() || st.bg_color.is_some()
                || !matches!(st.alignment, Alignment::Default) || st.bold)
            .collect();
        sheet_styles.push(resolved);
        sheet_conditionals.push(parse_sheet_conditionals(&sheet_xml, &dxfs));
    }
    Ok(WorkbookStyles { sheet_names, sheet_styles, sheet_conditionals })
}

fn read_xml(archive: &mut zip::ZipArchive<File>, name: &str) -> Option<String> {
    let mut file = archive.by_name(name).ok()?;
    let mut s = String::new();
    file.read_to_string(&mut s).ok()?;
    Some(s)
}

/// Parse hex `AARRGGBB` (Excel) or `RRGGBB` into an `RgbColor`. Returns None
/// for malformed strings. Alpha is ignored.
fn parse_argb(s: &str) -> Option<RgbColor> {
    let h = s.trim();
    let (r, g, b) = if h.len() == 8 {
        (
            u8::from_str_radix(&h[2..4], 16).ok()?,
            u8::from_str_radix(&h[4..6], 16).ok()?,
            u8::from_str_radix(&h[6..8], 16).ok()?,
        )
    } else if h.len() == 6 {
        (
            u8::from_str_radix(&h[0..2], 16).ok()?,
            u8::from_str_radix(&h[2..4], 16).ok()?,
            u8::from_str_radix(&h[4..6], 16).ok()?,
        )
    } else {
        return None;
    };
    Some((r, g, b))
}

#[derive(Clone, Default, Debug)]
struct FontInfo {
    color: Option<RgbColor>,
    bold: bool,
}

#[derive(Clone, Default, Debug)]
struct FillInfo {
    color: Option<RgbColor>,
}

#[derive(Clone, Default, Debug)]
struct CellXf {
    font_id: Option<usize>,
    fill_id: Option<usize>,
    align: Alignment,
    apply_font: bool,
    apply_fill: bool,
    apply_align: bool,
}

/// Parse xl/styles.xml. Returns one `CellStyle` per `cellXfs/xf` entry,
/// with font / fill / alignment resolved to concrete colors.
fn parse_styles(xml: &str) -> Vec<CellStyle> {
    let mut fonts: Vec<FontInfo> = Vec::new();
    let mut fills: Vec<FillInfo> = Vec::new();
    let mut cell_xfs: Vec<CellXf> = Vec::new();

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    #[derive(PartialEq)]
    enum Ctx { None, Fonts, Font, FontColor, Fills, Fill, PatternFill, CellXfs, Xf, Alignment }
    let mut ctx_stack: Vec<Ctx> = vec![Ctx::None];
    let mut current_font = FontInfo::default();
    let mut current_fill = FillInfo::default();

    while let Ok(ev) = reader.read_event() {
        let is_empty = matches!(ev, Event::Empty(_));
        match ev {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => {
                let name_owned = e.name().0.to_vec();
                let name = std::str::from_utf8(&name_owned).unwrap_or("");
                let local = name.rsplit_once(':').map(|(_, l)| l).unwrap_or(name);
                match local {
                    "fonts" => ctx_stack.push(Ctx::Fonts),
                    "font" if matches!(ctx_stack.last(), Some(Ctx::Fonts)) => {
                        current_font = FontInfo::default();
                        if !is_empty { ctx_stack.push(Ctx::Font); }
                    }
                    "color" if matches!(ctx_stack.last(), Some(Ctx::Font)) => {
                        for a in e.attributes().flatten() {
                            if a.key.0 == b"rgb" {
                                if let Ok(v) = std::str::from_utf8(&a.value) {
                                    current_font.color = parse_argb(v);
                                }
                            }
                        }
                        if !is_empty { ctx_stack.push(Ctx::FontColor); }
                    }
                    "b" if matches!(ctx_stack.last(), Some(Ctx::Font)) => {
                        // <b/> means bold = true. <b val="0"/> means false.
                        let mut on = true;
                        for a in e.attributes().flatten() {
                            if a.key.0 == b"val" {
                                if let Ok(v) = std::str::from_utf8(&a.value) {
                                    on = v != "0" && v.to_lowercase() != "false";
                                }
                            }
                        }
                        current_font.bold = on;
                    }
                    "fills" => ctx_stack.push(Ctx::Fills),
                    "fill" if matches!(ctx_stack.last(), Some(Ctx::Fills)) => {
                        current_fill = FillInfo::default();
                        if !is_empty { ctx_stack.push(Ctx::Fill); }
                    }
                    "patternFill" if matches!(ctx_stack.last(), Some(Ctx::Fill)) => {
                        if !is_empty { ctx_stack.push(Ctx::PatternFill); }
                    }
                    "fgColor" if matches!(ctx_stack.last(), Some(Ctx::PatternFill)) => {
                        for a in e.attributes().flatten() {
                            if a.key.0 == b"rgb" {
                                if let Ok(v) = std::str::from_utf8(&a.value) {
                                    current_fill.color = parse_argb(v);
                                }
                            }
                        }
                    }
                    "cellXfs" => ctx_stack.push(Ctx::CellXfs),
                    "xf" if matches!(ctx_stack.last(), Some(Ctx::CellXfs)) => {
                        let mut xf = CellXf::default();
                        for a in e.attributes().flatten() {
                            let v = std::str::from_utf8(&a.value).unwrap_or("");
                            match a.key.0 {
                                b"fontId" => xf.font_id = v.parse().ok(),
                                b"fillId" => xf.fill_id = v.parse().ok(),
                                b"applyFont" => xf.apply_font = v == "1",
                                b"applyFill" => xf.apply_fill = v == "1",
                                b"applyAlignment" => xf.apply_align = v == "1",
                                _ => {}
                            }
                        }
                        cell_xfs.push(xf);
                        if !is_empty { ctx_stack.push(Ctx::Xf); }
                    }
                    "alignment" if matches!(ctx_stack.last(), Some(Ctx::Xf)) => {
                        if let Some(last) = cell_xfs.last_mut() {
                            for a in e.attributes().flatten() {
                                if a.key.0 == b"horizontal" {
                                    if let Ok(v) = std::str::from_utf8(&a.value) {
                                        last.align = match v {
                                            "left" => Alignment::Left,
                                            "center" | "centerContinuous" => Alignment::Center,
                                            "right" => Alignment::Right,
                                            _ => Alignment::Default,
                                        };
                                    }
                                }
                            }
                        }
                        if !is_empty { ctx_stack.push(Ctx::Alignment); }
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                let name_owned = e.name().0.to_vec();
                let name = std::str::from_utf8(&name_owned).unwrap_or("");
                let local = name.rsplit_once(':').map(|(_, l)| l).unwrap_or(name);
                match local {
                    "fonts" if matches!(ctx_stack.last(), Some(Ctx::Fonts)) => { ctx_stack.pop(); }
                    "font" if matches!(ctx_stack.last(), Some(Ctx::Font)) => {
                        fonts.push(current_font.clone());
                        ctx_stack.pop();
                    }
                    "color" if matches!(ctx_stack.last(), Some(Ctx::FontColor)) => { ctx_stack.pop(); }
                    "fills" if matches!(ctx_stack.last(), Some(Ctx::Fills)) => { ctx_stack.pop(); }
                    "fill" if matches!(ctx_stack.last(), Some(Ctx::Fill)) => {
                        fills.push(current_fill.clone());
                        ctx_stack.pop();
                    }
                    "patternFill" if matches!(ctx_stack.last(), Some(Ctx::PatternFill)) => { ctx_stack.pop(); }
                    "cellXfs" if matches!(ctx_stack.last(), Some(Ctx::CellXfs)) => { ctx_stack.pop(); }
                    "xf" if matches!(ctx_stack.last(), Some(Ctx::Xf)) => { ctx_stack.pop(); }
                    "alignment" if matches!(ctx_stack.last(), Some(Ctx::Alignment)) => { ctx_stack.pop(); }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // Resolve each cell_xf into a CellStyle. Fonts[0] / fills[0] / fills[1]
    // are Excel defaults; we only apply colors when applyFont/applyFill is
    // set, matching Excel's evaluation rule.
    cell_xfs.into_iter().map(|xf| {
        let mut st = CellStyle::default();
        if xf.apply_font {
            if let Some(fi) = xf.font_id.and_then(|i| fonts.get(i)) {
                st.font_color = fi.color;
                st.bold = fi.bold;
            }
        }
        if xf.apply_fill {
            if let Some(fi) = xf.fill_id.and_then(|i| fills.get(i)) {
                st.bg_color = fi.color;
            }
        }
        if xf.apply_align {
            st.alignment = xf.align;
        }
        // Even without applyFont, font defaults still carry bold sometimes
        // (Excel auto-applies). Be permissive: take bold/color if applyFont
        // is missing but font has bold or non-default color.
        if !xf.apply_font {
            if let Some(fi) = xf.font_id.and_then(|i| fonts.get(i)) {
                if fi.bold { st.bold = true; }
            }
        }
        st
    }).collect()
}

/// Parse xl/workbook.xml + xl/_rels/workbook.xml.rels to get the workbook-
/// ordered list of (sheet_name, sheet_xml_path) pairs.
fn parse_workbook(xml: &str, rels_xml: &str) -> Vec<(String, String)> {
    // First, collect (id -> target path) from rels.
    let mut id_to_target: HashMap<String, String> = HashMap::new();
    let mut reader = Reader::from_str(rels_xml);
    reader.config_mut().trim_text(true);
    while let Ok(ev) = reader.read_event() {
        match ev {
            Event::Eof => break,
            Event::Empty(e) | Event::Start(e) => {
                let name_owned = e.name().0.to_vec();
                let name = std::str::from_utf8(&name_owned).unwrap_or("");
                let local = name.rsplit_once(':').map(|(_, l)| l).unwrap_or(name);
                if local == "Relationship" {
                    let mut id = String::new();
                    let mut target = String::new();
                    for a in e.attributes().flatten() {
                        match a.key.0 {
                            b"Id" => id = std::str::from_utf8(&a.value).unwrap_or("").to_string(),
                            b"Target" => target = std::str::from_utf8(&a.value).unwrap_or("").to_string(),
                            _ => {}
                        }
                    }
                    if !id.is_empty() && !target.is_empty() {
                        id_to_target.insert(id, target);
                    }
                }
            }
            _ => {}
        }
    }

    // Walk workbook.xml's <sheets><sheet name="X" r:id="rId1"/> in order.
    let mut out = Vec::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    while let Ok(ev) = reader.read_event() {
        match ev {
            Event::Eof => break,
            Event::Empty(e) | Event::Start(e) => {
                let name_owned = e.name().0.to_vec();
                let name = std::str::from_utf8(&name_owned).unwrap_or("");
                let local = name.rsplit_once(':').map(|(_, l)| l).unwrap_or(name);
                if local == "sheet" {
                    let mut sname = String::new();
                    let mut rid = String::new();
                    for a in e.attributes().flatten() {
                        let v = std::str::from_utf8(&a.value).unwrap_or("");
                        let k = std::str::from_utf8(a.key.0).unwrap_or("");
                        let local_k = k.rsplit_once(':').map(|(_, l)| l).unwrap_or(k);
                        match local_k {
                            "name" => sname = v.to_string(),
                            "id" => rid = v.to_string(),
                            _ => {}
                        }
                    }
                    if let Some(target) = id_to_target.get(&rid) {
                        out.push((sname, target.clone()));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Walk a sheet XML and collect each cell's style index. Cells without
/// `s="N"` are skipped (they use the default style which we don't surface).
fn parse_sheet_cell_styles(xml: &str) -> HashMap<(usize, usize), usize> {
    let mut out = HashMap::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    while let Ok(ev) = reader.read_event() {
        match ev {
            Event::Eof => break,
            Event::Empty(e) | Event::Start(e) => {
                let name_owned = e.name().0.to_vec();
                let name = std::str::from_utf8(&name_owned).unwrap_or("");
                let local = name.rsplit_once(':').map(|(_, l)| l).unwrap_or(name);
                if local == "c" {
                    let mut r_attr = String::new();
                    let mut s_attr: Option<usize> = None;
                    for a in e.attributes().flatten() {
                        match a.key.0 {
                            b"r" => r_attr = std::str::from_utf8(&a.value).unwrap_or("").to_string(),
                            b"s" => s_attr = std::str::from_utf8(&a.value).unwrap_or("").parse().ok(),
                            _ => {}
                        }
                    }
                    if let (true, Some(s)) = (!r_attr.is_empty(), s_attr) {
                        if let Some((col, row, _, _)) = crate::formula::parse_cell_ref(&r_attr) {
                            out.insert((col, row), s);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Parse the `<dxfs>` palette from `xl/styles.xml`. Each dxf is a
/// differential format used by conditional rules; we only extract the
/// fill (bg) color and font color since that's what tbla applies.
fn parse_dxfs(xml: &str) -> Vec<CellStyle> {
    let mut dxfs: Vec<CellStyle> = Vec::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    #[derive(PartialEq)]
    enum Ctx { None, Dxfs, Dxf, Font, FontColor, Fill, PatternFill }
    let mut stack: Vec<Ctx> = vec![Ctx::None];
    let mut current = CellStyle::default();

    while let Ok(ev) = reader.read_event() {
        let is_empty = matches!(ev, Event::Empty(_));
        match ev {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => {
                let name_owned = e.name().0.to_vec();
                let name = std::str::from_utf8(&name_owned).unwrap_or("");
                let local = name.rsplit_once(':').map(|(_, l)| l).unwrap_or(name);
                match local {
                    "dxfs" => stack.push(Ctx::Dxfs),
                    "dxf" if matches!(stack.last(), Some(Ctx::Dxfs)) => {
                        current = CellStyle::default();
                        if !is_empty { stack.push(Ctx::Dxf); }
                    }
                    "font" if matches!(stack.last(), Some(Ctx::Dxf)) => {
                        if !is_empty { stack.push(Ctx::Font); }
                    }
                    "color" if matches!(stack.last(), Some(Ctx::Font)) => {
                        for a in e.attributes().flatten() {
                            if a.key.0 == b"rgb" {
                                if let Ok(v) = std::str::from_utf8(&a.value) {
                                    current.font_color = parse_argb(v);
                                }
                            }
                        }
                        if !is_empty { stack.push(Ctx::FontColor); }
                    }
                    "b" if matches!(stack.last(), Some(Ctx::Font)) => {
                        current.bold = true;
                    }
                    "fill" if matches!(stack.last(), Some(Ctx::Dxf)) => {
                        if !is_empty { stack.push(Ctx::Fill); }
                    }
                    "patternFill" if matches!(stack.last(), Some(Ctx::Fill)) => {
                        for a in e.attributes().flatten() {
                            // For dxf fills, bgColor is actually the bg, and
                            // fgColor is the fill color — Excel uses both
                            // interchangeably in conditional formats. We
                            // accept either as the bg the user sees.
                            if a.key.0 == b"patternType" { /* ignore type */ }
                        }
                        if !is_empty { stack.push(Ctx::PatternFill); }
                    }
                    "bgColor" | "fgColor" if matches!(stack.last(), Some(Ctx::PatternFill)) => {
                        for a in e.attributes().flatten() {
                            if a.key.0 == b"rgb" {
                                if let Ok(v) = std::str::from_utf8(&a.value) {
                                    if let Some(c) = parse_argb(v) { current.bg_color = Some(c); }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                let name_owned = e.name().0.to_vec();
                let name = std::str::from_utf8(&name_owned).unwrap_or("");
                let local = name.rsplit_once(':').map(|(_, l)| l).unwrap_or(name);
                match local {
                    "dxfs" if matches!(stack.last(), Some(Ctx::Dxfs)) => { stack.pop(); }
                    "dxf" if matches!(stack.last(), Some(Ctx::Dxf)) => {
                        dxfs.push(current.clone());
                        stack.pop();
                    }
                    "font" if matches!(stack.last(), Some(Ctx::Font)) => { stack.pop(); }
                    "color" if matches!(stack.last(), Some(Ctx::FontColor)) => { stack.pop(); }
                    "fill" if matches!(stack.last(), Some(Ctx::Fill)) => { stack.pop(); }
                    "patternFill" if matches!(stack.last(), Some(Ctx::PatternFill)) => { stack.pop(); }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    dxfs
}

/// Parse `<conditionalFormatting>` elements from a single sheet's XML and
/// return one `ConditionalFormat` per (rule × range) combination. The dxf
/// palette is needed to resolve `dxfId` references to actual colors.
fn parse_sheet_conditionals(xml: &str, dxfs: &[CellStyle]) -> Vec<crate::sheet::ConditionalFormat> {
    use crate::sheet::{CondCondition, CondOp, ConditionalFormat};

    let mut out: Vec<ConditionalFormat> = Vec::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    #[derive(PartialEq)]
    enum Ctx { None, Cf, Rule, ColorScale, DataBar, Formula }
    let mut stack: Vec<Ctx> = vec![Ctx::None];

    // Current <conditionalFormatting> state
    let mut current_sqref = String::new();

    // Current <cfRule> state
    let mut rule_type = String::new();
    let mut rule_op = String::new();
    let mut rule_dxf_id: Option<usize> = None;
    let mut rule_formula = String::new();
    let mut scale_colors: Vec<crate::cell::RgbColor> = Vec::new();
    let mut bar_color: Option<crate::cell::RgbColor> = None;

    while let Ok(ev) = reader.read_event() {
        let is_empty = matches!(ev, Event::Empty(_));
        match ev {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => {
                let name_owned = e.name().0.to_vec();
                let name = std::str::from_utf8(&name_owned).unwrap_or("");
                let local = name.rsplit_once(':').map(|(_, l)| l).unwrap_or(name);
                match local {
                    "conditionalFormatting" => {
                        current_sqref.clear();
                        for a in e.attributes().flatten() {
                            if a.key.0 == b"sqref" {
                                if let Ok(v) = std::str::from_utf8(&a.value) {
                                    current_sqref = v.to_string();
                                }
                            }
                        }
                        if !is_empty { stack.push(Ctx::Cf); }
                    }
                    "cfRule" if matches!(stack.last(), Some(Ctx::Cf)) => {
                        rule_type.clear(); rule_op.clear(); rule_dxf_id = None;
                        rule_formula.clear(); scale_colors.clear(); bar_color = None;
                        for a in e.attributes().flatten() {
                            let v = std::str::from_utf8(&a.value).unwrap_or("");
                            match a.key.0 {
                                b"type" => rule_type = v.to_string(),
                                b"operator" => rule_op = v.to_string(),
                                b"dxfId" => rule_dxf_id = v.parse().ok(),
                                _ => {}
                            }
                        }
                        if !is_empty { stack.push(Ctx::Rule); }
                    }
                    "colorScale" if matches!(stack.last(), Some(Ctx::Rule)) => {
                        if !is_empty { stack.push(Ctx::ColorScale); }
                    }
                    "dataBar" if matches!(stack.last(), Some(Ctx::Rule)) => {
                        if !is_empty { stack.push(Ctx::DataBar); }
                    }
                    "color" if matches!(stack.last(), Some(Ctx::ColorScale)) => {
                        for a in e.attributes().flatten() {
                            if a.key.0 == b"rgb" {
                                if let Ok(v) = std::str::from_utf8(&a.value) {
                                    if let Some(c) = parse_argb(v) { scale_colors.push(c); }
                                }
                            }
                        }
                    }
                    "color" if matches!(stack.last(), Some(Ctx::DataBar)) => {
                        for a in e.attributes().flatten() {
                            if a.key.0 == b"rgb" {
                                if let Ok(v) = std::str::from_utf8(&a.value) {
                                    if let Some(c) = parse_argb(v) { bar_color = Some(c); }
                                }
                            }
                        }
                    }
                    "formula" if matches!(stack.last(), Some(Ctx::Rule)) => {
                        rule_formula.clear();
                        if !is_empty { stack.push(Ctx::Formula); }
                    }
                    _ => {}
                }
            }
            Event::Text(t) => {
                if matches!(stack.last(), Some(Ctx::Formula)) {
                    rule_formula.push_str(&t.unescape().unwrap_or_default());
                }
            }
            Event::End(e) => {
                let name_owned = e.name().0.to_vec();
                let name = std::str::from_utf8(&name_owned).unwrap_or("");
                let local = name.rsplit_once(':').map(|(_, l)| l).unwrap_or(name);
                match local {
                    "formula" if matches!(stack.last(), Some(Ctx::Formula)) => { stack.pop(); }
                    "colorScale" if matches!(stack.last(), Some(Ctx::ColorScale)) => { stack.pop(); }
                    "dataBar" if matches!(stack.last(), Some(Ctx::DataBar)) => { stack.pop(); }
                    "cfRule" if matches!(stack.last(), Some(Ctx::Rule)) => {
                        // Emit the rule for each sub-range in the sqref.
                        let (text_color, bg_color) = match rule_dxf_id.and_then(|i| dxfs.get(i)) {
                            Some(d) => (d.font_color, d.bg_color),
                            None => (None, None),
                        };
                        let condition = match rule_type.as_str() {
                            "cellIs" => {
                                let op = match rule_op.as_str() {
                                    "greaterThan" => CondOp::Gt,
                                    "lessThan" => CondOp::Lt,
                                    "greaterThanOrEqual" => CondOp::Ge,
                                    "lessThanOrEqual" => CondOp::Le,
                                    "equal" => CondOp::Eq,
                                    "notEqual" => CondOp::Ne,
                                    _ => { stack.pop(); continue; }
                                };
                                let target: f64 = match rule_formula.trim().parse() {
                                    Ok(n) => n,
                                    Err(_) => { stack.pop(); continue; }
                                };
                                Some(CondCondition::Compare { op, target })
                            }
                            "colorScale" if scale_colors.len() >= 2 => {
                                // tbla supports 2-color scales; Excel may
                                // emit a 3-color scale. Use first + last.
                                let lo = scale_colors[0];
                                let hi = *scale_colors.last().unwrap();
                                // Defer min/max to a runtime auto-range —
                                // we don't have it here. Use (0, 1) so the
                                // user can adjust; not perfect for first
                                // render but the scale still shows up.
                                Some(CondCondition::ColorScale { min: 0.0, max: 1.0, min_color: lo, max_color: hi })
                            }
                            "dataBar" => {
                                Some(CondCondition::DataBar {
                                    min: None, max: None,
                                    bar_color: bar_color.unwrap_or((99, 142, 198)),
                                })
                            }
                            _ => None,
                        };
                        if let Some(condition) = condition {
                            for sub in current_sqref.split_ascii_whitespace() {
                                if let Some(r) = parse_sqref_range(sub) {
                                    out.push(ConditionalFormat {
                                        min_col: r.0, min_row: r.1,
                                        max_col: r.2, max_row: r.3,
                                        condition: condition.clone(),
                                        text_color, bg_color,
                                    });
                                }
                            }
                        }
                        stack.pop();
                    }
                    "conditionalFormatting" if matches!(stack.last(), Some(Ctx::Cf)) => { stack.pop(); }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

fn parse_sqref_range(s: &str) -> Option<(usize, usize, usize, usize)> {
    if let Some((a, b)) = s.split_once(':') {
        let (c1, r1, _, _) = crate::formula::parse_cell_ref(a.trim())?;
        let (c2, r2, _, _) = crate::formula::parse_cell_ref(b.trim())?;
        Some((c1.min(c2), r1.min(r2), c1.max(c2), r1.max(r2)))
    } else {
        let (c, r, _, _) = crate::formula::parse_cell_ref(s.trim())?;
        Some((c, r, c, r))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_argb_basic() {
        assert_eq!(parse_argb("FFFF0000"), Some((255, 0, 0))); // alpha+red
        assert_eq!(parse_argb("00FF00"), Some((0, 255, 0)));
        assert_eq!(parse_argb("xxx"), None);
    }
}
