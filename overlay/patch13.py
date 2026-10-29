import io

# ---- widget.rs: the JSON may rename the section header ------------------------
p = 'src/widget.rs'
s = io.open(p, encoding='utf-8', newline='').read()
if 'pub title: Option<String>' not in s.split('pub struct CustomView')[1][:400]:
    s = s.replace("pub struct CustomView {\n    pub rows: Vec<CustomRowView>,",
                  "pub struct CustomView {\n    /// Overrides the section header when the JSON supplies one.\n    pub title: Option<String>,\n    pub rows: Vec<CustomRowView>,")
    io.open(p, 'w', encoding='utf-8', newline='').write(s)
    print('widget.rs: title added')

# ---- main.rs: fill it from the spec ------------------------------------------
p = 'src/main.rs'
s = io.open(p, encoding='utf-8', newline='').read()
if 'title: st.custom.title.clone(),' not in s:
    s = s.replace("    let mut v = widget::CustomView {\n        error: st.custom.error.clone(),",
                  "    let mut v = widget::CustomView {\n        title: st.custom.title.clone(),\n        error: st.custom.error.clone(),")
    io.open(p, 'w', encoding='utf-8', newline='').write(s)
    print('main.rs: title wired')

# ---- sections.rs: use it for the custom header -------------------------------
p = 'src/sections.rs'
s = io.open(p, encoding='utf-8', newline='').read()
anchor = "        out.push(\n            Hit::new(\n                rect(PAD, y, content_right(), y + SECTION_TITLE_H),"
if anchor in s and 'let title = if sec.key == "custom"' not in s:
    s = s.replace(anchor,
                  "        let title = if sec.key == \"custom\" {\n"
                  "            v.custom.title.clone().unwrap_or_else(|| sec.title.to_string())\n"
                  "        } else {\n"
                  "            sec.title.to_string()\n"
                  "        };\n" + anchor, 1)
    s = s.replace(", sec.title))", ", title))", 1)
    io.open(p, 'w', encoding='utf-8', newline='').write(s)
    print('sections.rs: header uses the custom title')
