use unicode_width::UnicodeWidthStr;

/// Actions dispatchable from menus and keyboard shortcuts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    FileNew,
    FileOpen,
    FileSave,
    FileSaveAs,
    FileImportCsv,
    FileExportCsv,
    FileOpenCsvAsDf,
    FileSaveParquet,
    FilePrintHtml,
    FileQuit,

    EditUndo,
    EditRedo,
    EditCopy,
    EditCut,
    EditPaste,
    EditClear,
    EditSelectAll,
    EditFind,
    EditFindNext,
    EditFindPrev,
    EditReplace,
    EditGoto,

    InsertRow,
    InsertCol,
    DeleteRow,
    DeleteCol,

    DataSort,
    DataFilter,
    DataFilterClear,
    DataToDataframe,
    DataToCells,
    DataAddComputed,
    DataClearComputed,
    DataSqlQuery,
    DataGroupBy,

    SheetNew,
    SheetDelete,
    SheetRename,
    SheetNext,
    SheetPrev,

    FormatAutoWidth,
    FormatWiderCol,
    FormatNarrowerCol,
    FormatSetWidth,
    FormatBoldToggle,
    FormatAlignLeft,
    FormatAlignCenter,
    FormatAlignRight,
    FormatAlignDefault,
    FormatTextColor,
    FormatBgColor,
    FormatNumber,
    FormatClear,
    FormatConditional,
    FormatConditionalClear,

    HelpKeys,
    HelpAbout,
}

#[derive(Clone, Debug)]
pub enum SubItem {
    Separator,
    Item {
        label: String,
        mnemonic: Option<char>,
        shortcut: Option<&'static str>,
        action: Action,
    },
}

impl SubItem {
    pub fn is_separator(&self) -> bool {
        matches!(self, SubItem::Separator)
    }
}

#[derive(Clone, Debug)]
pub struct TopMenu {
    pub label: String,
    pub mnemonic: char,
    pub items: Vec<SubItem>,
}

pub struct MenuBar {
    pub menus: Vec<TopMenu>,
}

impl Default for MenuBar {
    fn default() -> Self {
        MenuBar::new()
    }
}

impl MenuBar {
    pub fn new() -> Self {
        let menus = vec![
            TopMenu {
                label: "ファイル(F)".to_string(),
                mnemonic: 'F',
                items: vec![
                    SubItem::Item {
                        label: "新規".to_string(),
                        mnemonic: Some('N'),
                        shortcut: Some("Ctrl+N"),
                        action: Action::FileNew,
                    },
                    SubItem::Item {
                        label: "開く...".to_string(),
                        mnemonic: Some('O'),
                        shortcut: Some("Ctrl+O"),
                        action: Action::FileOpen,
                    },
                    SubItem::Item {
                        label: "保存".to_string(),
                        mnemonic: Some('S'),
                        shortcut: Some("Ctrl+S"),
                        action: Action::FileSave,
                    },
                    SubItem::Item {
                        label: "名前を付けて保存...".to_string(),
                        mnemonic: Some('A'),
                        shortcut: None,
                        action: Action::FileSaveAs,
                    },
                    SubItem::Separator,
                    SubItem::Item {
                        label: "CSVインポート...".to_string(),
                        mnemonic: Some('I'),
                        shortcut: None,
                        action: Action::FileImportCsv,
                    },
                    SubItem::Item {
                        label: "CSVエクスポート...".to_string(),
                        mnemonic: Some('E'),
                        shortcut: None,
                        action: Action::FileExportCsv,
                    },
                    SubItem::Separator,
                    SubItem::Item {
                        label: "CSV を DataFrame として開く...".to_string(),
                        mnemonic: Some('D'),
                        shortcut: None,
                        action: Action::FileOpenCsvAsDf,
                    },
                    SubItem::Item {
                        label: "Parquet として保存...".to_string(),
                        mnemonic: Some('Q'),
                        shortcut: None,
                        action: Action::FileSaveParquet,
                    },
                    SubItem::Separator,
                    SubItem::Item {
                        label: "印刷 (HTML)...".to_string(),
                        mnemonic: Some('P'),
                        shortcut: Some("Ctrl+P"),
                        action: Action::FilePrintHtml,
                    },
                    SubItem::Separator,
                    SubItem::Item {
                        label: "終了".to_string(),
                        mnemonic: Some('X'),
                        shortcut: Some("Ctrl+Q"),
                        action: Action::FileQuit,
                    },
                ],
            },
            TopMenu {
                label: "編集(E)".to_string(),
                mnemonic: 'E',
                items: vec![
                    SubItem::Item {
                        label: "元に戻す".to_string(),
                        mnemonic: Some('U'),
                        shortcut: Some("Ctrl+Z"),
                        action: Action::EditUndo,
                    },
                    SubItem::Item {
                        label: "やり直し".to_string(),
                        mnemonic: Some('R'),
                        shortcut: Some("Ctrl+Y"),
                        action: Action::EditRedo,
                    },
                    SubItem::Separator,
                    SubItem::Item {
                        label: "切り取り".to_string(),
                        mnemonic: Some('T'),
                        shortcut: Some("Ctrl+X"),
                        action: Action::EditCut,
                    },
                    SubItem::Item {
                        label: "コピー".to_string(),
                        mnemonic: Some('C'),
                        shortcut: Some("Ctrl+C"),
                        action: Action::EditCopy,
                    },
                    SubItem::Item {
                        label: "貼り付け".to_string(),
                        mnemonic: Some('P'),
                        shortcut: Some("Ctrl+V"),
                        action: Action::EditPaste,
                    },
                    SubItem::Item {
                        label: "クリア".to_string(),
                        mnemonic: Some('L'),
                        shortcut: Some("Delete"),
                        action: Action::EditClear,
                    },
                    SubItem::Separator,
                    SubItem::Item {
                        label: "すべて選択".to_string(),
                        mnemonic: Some('A'),
                        shortcut: Some("Ctrl+A"),
                        action: Action::EditSelectAll,
                    },
                    SubItem::Item {
                        label: "検索...".to_string(),
                        mnemonic: Some('F'),
                        shortcut: Some("Ctrl+F"),
                        action: Action::EditFind,
                    },
                    SubItem::Item {
                        label: "次を検索".to_string(),
                        mnemonic: Some('N'),
                        shortcut: Some("F3"),
                        action: Action::EditFindNext,
                    },
                    SubItem::Item {
                        label: "置換...".to_string(),
                        mnemonic: Some('R'),
                        shortcut: Some("Ctrl+R"),
                        action: Action::EditReplace,
                    },
                    SubItem::Item {
                        label: "ジャンプ...".to_string(),
                        mnemonic: Some('G'),
                        shortcut: Some("Ctrl+G"),
                        action: Action::EditGoto,
                    },
                ],
            },
            TopMenu {
                label: "挿入(I)".to_string(),
                mnemonic: 'I',
                items: vec![
                    SubItem::Item {
                        label: "行を挿入".to_string(),
                        mnemonic: Some('R'),
                        shortcut: None,
                        action: Action::InsertRow,
                    },
                    SubItem::Item {
                        label: "列を挿入".to_string(),
                        mnemonic: Some('C'),
                        shortcut: None,
                        action: Action::InsertCol,
                    },
                    SubItem::Separator,
                    SubItem::Item {
                        label: "行を削除".to_string(),
                        mnemonic: Some('D'),
                        shortcut: None,
                        action: Action::DeleteRow,
                    },
                    SubItem::Item {
                        label: "列を削除".to_string(),
                        mnemonic: Some('E'),
                        shortcut: None,
                        action: Action::DeleteCol,
                    },
                ],
            },
            TopMenu {
                label: "シート(S)".to_string(),
                mnemonic: 'S',
                items: vec![
                    SubItem::Item {
                        label: "新規シート".to_string(),
                        mnemonic: Some('N'),
                        shortcut: None,
                        action: Action::SheetNew,
                    },
                    SubItem::Item {
                        label: "シート名変更...".to_string(),
                        mnemonic: Some('R'),
                        shortcut: None,
                        action: Action::SheetRename,
                    },
                    SubItem::Item {
                        label: "シート削除".to_string(),
                        mnemonic: Some('D'),
                        shortcut: None,
                        action: Action::SheetDelete,
                    },
                    SubItem::Separator,
                    SubItem::Item {
                        label: "次のシート".to_string(),
                        mnemonic: Some('X'),
                        shortcut: Some("Ctrl+PgDn"),
                        action: Action::SheetNext,
                    },
                    SubItem::Item {
                        label: "前のシート".to_string(),
                        mnemonic: Some('V'),
                        shortcut: Some("Ctrl+PgUp"),
                        action: Action::SheetPrev,
                    },
                ],
            },
            TopMenu {
                label: "データ(D)".to_string(),
                mnemonic: 'D',
                items: vec![
                    SubItem::Item {
                        label: "並べ替え...".to_string(),
                        mnemonic: Some('S'),
                        shortcut: None,
                        action: Action::DataSort,
                    },
                    SubItem::Item {
                        label: "フィルター...".to_string(),
                        mnemonic: Some('F'),
                        shortcut: None,
                        action: Action::DataFilter,
                    },
                    SubItem::Item {
                        label: "フィルター解除".to_string(),
                        mnemonic: Some('C'),
                        shortcut: None,
                        action: Action::DataFilterClear,
                    },
                    SubItem::Separator,
                    SubItem::Item {
                        label: "DataFrame ビューに変換".to_string(),
                        mnemonic: Some('V'),
                        shortcut: None,
                        action: Action::DataToDataframe,
                    },
                    SubItem::Item {
                        label: "セルビューに戻す".to_string(),
                        mnemonic: Some('B'),
                        shortcut: None,
                        action: Action::DataToCells,
                    },
                    SubItem::Item {
                        label: "計算列を追加...".to_string(),
                        mnemonic: Some('A'),
                        shortcut: None,
                        action: Action::DataAddComputed,
                    },
                    SubItem::Item {
                        label: "計算列をクリア".to_string(),
                        mnemonic: Some('E'),
                        shortcut: None,
                        action: Action::DataClearComputed,
                    },
                    SubItem::Separator,
                    SubItem::Item {
                        label: "SQL クエリ...".to_string(),
                        mnemonic: Some('Q'),
                        shortcut: None,
                        action: Action::DataSqlQuery,
                    },
                    SubItem::Item {
                        label: "グループ集計...".to_string(),
                        mnemonic: Some('G'),
                        shortcut: None,
                        action: Action::DataGroupBy,
                    },
                ],
            },
            TopMenu {
                label: "書式(O)".to_string(),
                mnemonic: 'O',
                items: vec![
                    SubItem::Item {
                        label: "列幅を自動調整".to_string(),
                        mnemonic: Some('A'),
                        shortcut: None,
                        action: Action::FormatAutoWidth,
                    },
                    SubItem::Item {
                        label: "列幅を広げる".to_string(),
                        mnemonic: Some('W'),
                        shortcut: None,
                        action: Action::FormatWiderCol,
                    },
                    SubItem::Item {
                        label: "列幅を狭める".to_string(),
                        mnemonic: Some('N'),
                        shortcut: None,
                        action: Action::FormatNarrowerCol,
                    },
                    SubItem::Item {
                        label: "列幅を変更...".to_string(),
                        mnemonic: Some('S'),
                        shortcut: None,
                        action: Action::FormatSetWidth,
                    },
                    SubItem::Separator,
                    SubItem::Item {
                        label: "太字 切替".to_string(),
                        mnemonic: Some('B'),
                        shortcut: Some("Ctrl+B"),
                        action: Action::FormatBoldToggle,
                    },
                    SubItem::Item {
                        label: "左揃え".to_string(),
                        mnemonic: Some('L'),
                        shortcut: None,
                        action: Action::FormatAlignLeft,
                    },
                    SubItem::Item {
                        label: "中央揃え".to_string(),
                        mnemonic: Some('C'),
                        shortcut: None,
                        action: Action::FormatAlignCenter,
                    },
                    SubItem::Item {
                        label: "右揃え".to_string(),
                        mnemonic: Some('R'),
                        shortcut: None,
                        action: Action::FormatAlignRight,
                    },
                    SubItem::Item {
                        label: "揃えを既定に戻す".to_string(),
                        mnemonic: Some('D'),
                        shortcut: None,
                        action: Action::FormatAlignDefault,
                    },
                    SubItem::Separator,
                    SubItem::Item {
                        label: "文字色...".to_string(),
                        mnemonic: Some('F'),
                        shortcut: None,
                        action: Action::FormatTextColor,
                    },
                    SubItem::Item {
                        label: "背景色...".to_string(),
                        mnemonic: Some('G'),
                        shortcut: None,
                        action: Action::FormatBgColor,
                    },
                    SubItem::Item {
                        label: "数値書式...".to_string(),
                        mnemonic: Some('U'),
                        shortcut: None,
                        action: Action::FormatNumber,
                    },
                    SubItem::Item {
                        label: "書式クリア".to_string(),
                        mnemonic: Some('X'),
                        shortcut: None,
                        action: Action::FormatClear,
                    },
                    SubItem::Separator,
                    SubItem::Item {
                        label: "条件付き書式...".to_string(),
                        mnemonic: Some('T'),
                        shortcut: None,
                        action: Action::FormatConditional,
                    },
                    SubItem::Item {
                        label: "条件付き書式 全クリア".to_string(),
                        mnemonic: Some('Z'),
                        shortcut: None,
                        action: Action::FormatConditionalClear,
                    },
                ],
            },
            TopMenu {
                label: "ヘルプ(H)".to_string(),
                mnemonic: 'H',
                items: vec![
                    SubItem::Item {
                        label: "キー操作一覧".to_string(),
                        mnemonic: Some('K'),
                        shortcut: None,
                        action: Action::HelpKeys,
                    },
                    SubItem::Item {
                        label: "バージョン情報".to_string(),
                        mnemonic: Some('A'),
                        shortcut: None,
                        action: Action::HelpAbout,
                    },
                ],
            },
        ];

        MenuBar { menus }
    }

    /// Compute starting column for each top-level menu label on the menu bar.
    pub fn bar_positions(&self) -> Vec<(usize, usize)> {
        let mut positions = Vec::new();
        let mut x: usize = 1; // leading space
        for menu in &self.menus {
            let width = UnicodeWidthStr::width(menu.label.as_str());
            positions.push((x, width));
            x += width + 2; // 2-char gap
        }
        positions
    }

    /// Hit-test the menu bar: returns the index of the top-level menu at column.
    pub fn hit_test_bar(&self, col: u16) -> Option<usize> {
        let col = col as usize;
        for (idx, (x, w)) in self.bar_positions().into_iter().enumerate() {
            if col >= x && col < x + w {
                return Some(idx);
            }
        }
        None
    }

    /// Try to activate (open or move to) a top-level menu by mnemonic letter.
    pub fn activate_by_mnemonic(&self, c: char, state: &mut MenuState) -> bool {
        let upper = c.to_ascii_uppercase();
        for (idx, menu) in self.menus.iter().enumerate() {
            if menu.mnemonic.to_ascii_uppercase() == upper {
                state.open_index(idx);
                return true;
            }
        }
        false
    }

    /// Width of the widest item in a submenu (for rendering).
    pub fn submenu_width(&self, menu_idx: usize) -> usize {
        let menu = &self.menus[menu_idx];
        let mut max = 0;
        for item in &menu.items {
            if let SubItem::Item { label, shortcut, .. } = item {
                let mut w = UnicodeWidthStr::width(label.as_str()) + 4; // padding
                if let Some(s) = shortcut {
                    w += UnicodeWidthStr::width(*s) + 3;
                }
                if w > max {
                    max = w;
                }
            }
        }
        max.max(20)
    }
}

#[derive(Default)]
pub struct MenuState {
    pub open: Option<usize>,    // open top-level menu index
    pub item: usize,            // highlighted submenu item index
}

impl MenuState {
    pub fn close(&mut self) {
        self.open = None;
        self.item = 0;
    }

    pub fn open_first(&mut self) {
        self.open = Some(0);
        self.item = 0;
    }

    pub fn open_index(&mut self, idx: usize) {
        self.open = Some(idx);
        self.item = 0;
    }

    pub fn move_left(&mut self, bar: &MenuBar) {
        if let Some(idx) = self.open {
            let new_idx = if idx == 0 { bar.menus.len() - 1 } else { idx - 1 };
            self.open = Some(new_idx);
            self.item = 0;
        }
    }

    pub fn move_right(&mut self, bar: &MenuBar) {
        if let Some(idx) = self.open {
            let new_idx = (idx + 1) % bar.menus.len();
            self.open = Some(new_idx);
            self.item = 0;
        }
    }

    pub fn move_up(&mut self, bar: &MenuBar) {
        if let Some(idx) = self.open {
            let items = &bar.menus[idx].items;
            if items.is_empty() {
                return;
            }
            for _ in 0..items.len() {
                self.item = if self.item == 0 { items.len() - 1 } else { self.item - 1 };
                if !items[self.item].is_separator() {
                    break;
                }
            }
        }
    }

    pub fn move_down(&mut self, bar: &MenuBar) {
        if let Some(idx) = self.open {
            let items = &bar.menus[idx].items;
            if items.is_empty() {
                return;
            }
            for _ in 0..items.len() {
                self.item = (self.item + 1) % items.len();
                if !items[self.item].is_separator() {
                    break;
                }
            }
        }
    }

    /// Return the action for the highlighted item, if any.
    pub fn activate(&self, bar: &MenuBar) -> Option<Action> {
        let idx = self.open?;
        match bar.menus[idx].items.get(self.item)? {
            SubItem::Item { action, .. } => Some(*action),
            SubItem::Separator => None,
        }
    }

    /// Try to activate a submenu item by mnemonic letter.
    pub fn activate_by_mnemonic(&mut self, bar: &MenuBar, c: char) -> Option<Action> {
        let idx = self.open?;
        let upper = c.to_ascii_uppercase();
        for item in &bar.menus[idx].items {
            if let SubItem::Item { mnemonic: Some(m), action, .. } = item {
                if m.to_ascii_uppercase() == upper {
                    return Some(*action);
                }
            }
        }
        None
    }

    /// Hit-test the open submenu against a screen position.
    pub fn hit_test(&self, bar: &MenuBar, col: u16, row: u16) -> Option<Action> {
        let idx = self.open?;
        let positions = bar.bar_positions();
        let (x_start, _) = positions[idx];
        let width = bar.submenu_width(idx);

        let col = col as usize;
        let row = row as usize;

        // Submenu top-left at (x_start, 1)
        if col < x_start || col >= x_start + width {
            return None;
        }
        if row < 1 {
            return None;
        }
        let item_idx = row - 1;
        if item_idx >= bar.menus[idx].items.len() {
            return None;
        }
        match &bar.menus[idx].items[item_idx] {
            SubItem::Item { action, .. } => Some(*action),
            SubItem::Separator => None,
        }
    }
}

/// Right-click context menu state.
pub struct ContextMenu {
    pub col: u16,
    pub row: u16,
    pub items: Vec<(String, Action)>,
    pub selected: usize,
    pub width: usize,
}

impl ContextMenu {
    pub fn new(click_col: u16, click_row: u16, term_width: u16, term_height: u16) -> Self {
        let items: Vec<(String, Action)> = vec![
            ("切り取り       Ctrl+X".to_string(), Action::EditCut),
            ("コピー         Ctrl+C".to_string(), Action::EditCopy),
            ("貼り付け       Ctrl+V".to_string(), Action::EditPaste),
            ("クリア         Delete".to_string(), Action::EditClear),
            ("行を挿入".to_string(), Action::InsertRow),
            ("列を挿入".to_string(), Action::InsertCol),
            ("行を削除".to_string(), Action::DeleteRow),
            ("列を削除".to_string(), Action::DeleteCol),
            ("列幅を自動調整".to_string(), Action::FormatAutoWidth),
            ("列幅を変更...".to_string(), Action::FormatSetWidth),
        ];

        let width = items.iter().map(|(s, _)| UnicodeWidthStr::width(s.as_str())).max().unwrap_or(20) + 4;

        // Adjust position so the menu fits within terminal
        let mut col = click_col;
        let mut row = click_row;
        if col as usize + width > term_width as usize {
            col = (term_width as usize).saturating_sub(width) as u16;
        }
        if row as usize + items.len() + 2 > term_height as usize {
            row = (term_height as usize).saturating_sub(items.len() + 2) as u16;
        }

        ContextMenu {
            col,
            row,
            items,
            selected: 0,
            width,
        }
    }

    pub fn move_up(&mut self) {
        if self.selected == 0 {
            self.selected = self.items.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        self.selected = (self.selected + 1) % self.items.len();
    }

    pub fn activate(&self) -> Option<Action> {
        self.items.get(self.selected).map(|(_, a)| *a)
    }

    pub fn hit_test(&self, col: u16, row: u16) -> Option<Action> {
        let c = col as usize;
        let r = row as usize;
        let mx = self.col as usize;
        let my = self.row as usize;

        // Menu has top/bottom borders at my and my+items+1, items at my+1..my+items
        if c < mx || c >= mx + self.width {
            return None;
        }
        if r <= my || r > my + self.items.len() {
            return None;
        }
        let idx = r - my - 1;
        self.items.get(idx).map(|(_, a)| *a)
    }
}
