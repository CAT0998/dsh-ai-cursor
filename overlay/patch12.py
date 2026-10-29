import io

# ---- custom.rs ---------------------------------------------------------------
p = 'src/custom.rs'
s = io.open(p, encoding='utf-8', newline='').read()
s = s.replace("""                "button" => {
                    let Some(action) = item.get("action").and_then(|a| a.as_str()) else {
                        if item.get("set").is_none() {
                            return Err(format!("第 {i} 行是 button，但既没有 action 也没有 set"));
                        }
                        String::new()
                    };
                    rows.push(CustomRow::Button {
                        text,
                        action,
                        set: item.get("set").cloned(),
                    });
                }""",
"""                "button" => {
                    let action = item
                        .get("action")
                        .and_then(|a| a.as_str())
                        .unwrap_or("")
                        .to_string();
                    let set = item.get("set").cloned();
                    if action.is_empty() && set.is_none() {
                        return Err(format!("第 {i} 行是 button，但既没有 action 也没有 set"));
                    }
                    rows.push(CustomRow::Button { text, action, set });
                }""")
io.open(p, 'w', encoding='utf-8', newline='').write(s)
print('custom.rs fixed')

# ---- widget.rs: PanelView needs the sprite timeline --------------------------
p = 'src/widget.rs'
s = io.open(p, encoding='utf-8', newline='').read()
s = s.replace("    /// The reserved custom section's current contents.\n    pub custom: CustomView,",
              "    /// The reserved custom section's current contents.\n    pub custom: CustomView,\n    /// Milliseconds into the sprite timeline; drives thumbnail animation.\n    pub epoch_ms: u64,")
io.open(p, 'w', encoding='utf-8', newline='').write(s)
print('widget.rs epoch added')

# ---- main.rs -----------------------------------------------------------------
p = 'src/main.rs'
s = io.open(p, encoding='utf-8', newline='').read()
applied, missed = [], []


def rep(tag, old, new):
    global s
    if old in s:
        s = s.replace(old, new, 1)
        applied.append(tag)
    else:
        missed.append(tag)


rep('view-epoch', "        custom: custom_view(st),\n    }\n}",
    "        custom: custom_view(st),\n        epoch_ms: Instant::now().duration_since(st.sprite_epoch).as_millis() as u64,\n    }\n}")

rep('draw-preview-kind', "            Kind::Wheel => {}",
    "            Kind::Wheel | Kind::Preview => {}")

rep('rank-preview', "            Kind::Label | Kind::LabelRight | Kind::Sep => 3,",
    "            Kind::Label | Kind::LabelRight | Kind::Sep | Kind::Preview => 3,")

io.open(p, 'w', encoding='utf-8', newline='').write(s)
print('applied:', applied)
print('MISSED :', missed)
