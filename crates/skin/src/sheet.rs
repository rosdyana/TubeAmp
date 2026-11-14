/// Bitmap sheets a classic skin can provide. Text files (`VISCOLOR.TXT`,
/// `PLEDIT.TXT`, `REGION.TXT`) are parsed separately and are not sheets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sheet {
    Main,
    Titlebar,
    Cbuttons,
    Numbers,
    NumsEx,
    Playpaus,
    Monoster,
    Posbar,
    Volume,
    Balance,
    Shufrep,
    Text,
    Pledit,
    Eqmain,
    EqEx,
    Gen,
    Genex,
    Video,
    Mb,
}

impl Sheet {
    /// Every sheet, in declaration order (`index()` matches this order).
    pub const ALL: [Sheet; 19] = [
        Sheet::Main,
        Sheet::Titlebar,
        Sheet::Cbuttons,
        Sheet::Numbers,
        Sheet::NumsEx,
        Sheet::Playpaus,
        Sheet::Monoster,
        Sheet::Posbar,
        Sheet::Volume,
        Sheet::Balance,
        Sheet::Shufrep,
        Sheet::Text,
        Sheet::Pledit,
        Sheet::Eqmain,
        Sheet::EqEx,
        Sheet::Gen,
        Sheet::Genex,
        Sheet::Video,
        Sheet::Mb,
    ];

    /// File name stem inside the archive (matched case-insensitively).
    pub const fn base_name(self) -> &'static str {
        match self {
            Sheet::Main => "MAIN",
            Sheet::Titlebar => "TITLEBAR",
            Sheet::Cbuttons => "CBUTTONS",
            Sheet::Numbers => "NUMBERS",
            Sheet::NumsEx => "NUMS_EX",
            Sheet::Playpaus => "PLAYPAUS",
            Sheet::Monoster => "MONOSTER",
            Sheet::Posbar => "POSBAR",
            Sheet::Volume => "VOLUME",
            Sheet::Balance => "BALANCE",
            Sheet::Shufrep => "SHUFREP",
            Sheet::Text => "TEXT",
            Sheet::Pledit => "PLEDIT",
            Sheet::Eqmain => "EQMAIN",
            Sheet::EqEx => "EQ_EX",
            Sheet::Gen => "GEN",
            Sheet::Genex => "GENEX",
            Sheet::Video => "VIDEO",
            Sheet::Mb => "MB",
        }
    }

    /// Position of this sheet in `Sheet::ALL`.
    pub const fn index(self) -> usize {
        self as usize
    }
}

#[cfg(test)]
mod tests {
    use super::Sheet;

    #[test]
    fn all_is_in_index_order_and_names_are_unique() {
        for (i, sheet) in Sheet::ALL.iter().enumerate() {
            assert_eq!(sheet.index(), i);
        }
        let mut names: Vec<&str> = Sheet::ALL.iter().map(|s| s.base_name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Sheet::ALL.len());
    }
}
