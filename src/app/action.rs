use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::state::{Focus, Mode, Overlay};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Action {
    None,
    Quit,
    Escape,
    ToggleHelp,
    ToggleMode,
    SetMode(Mode),
    CycleFocus(i8),
    Reload,
    ResizeTree(i16),

    TreeMove(isize),
    TreeHalfPage(i8),
    TreeFirst,
    TreeLast,
    TreeOpen,
    TreeCollapse,
    TreeToggle,
    ToggleIgnored,
    ToggleHierarchy,

    ContentScroll(isize),
    ContentHalfPage(i8),
    ContentPage(i8),
    ContentFirst,
    ContentLast,
    ContentHScroll(isize),
    NextHunk,
    PrevHunk,
    NextFile,
    PrevFile,
    ToggleFold,
    ExpandGap,
    ToggleUnified,
    ToggleWrap,
    CycleSort,
    CycleDiffSpec,
    Stage,
    Unstage,

    StartSearch,
    StartFilter,
    StartRange,
    InputChar(char),
    InputBackspace,
    InputSubmit,
    InputCancel,
    NextMatch,
    PrevMatch,
}

/// `]c` のような 2 打鍵を扱うため、直前の前置キーを保持する。
#[derive(Default)]
pub struct KeyMap {
    pending: Option<char>,
}

impl KeyMap {
    pub fn map(&mut self, key: KeyEvent, focus: Focus, overlay: &Overlay) -> Action {
        if key.kind == KeyEventKind::Release {
            return Action::None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('c')) {
            return Action::Quit;
        }

        if overlay.is_input() {
            self.pending = None;
            return match key.code {
                KeyCode::Esc => Action::InputCancel,
                KeyCode::Enter => Action::InputSubmit,
                KeyCode::Backspace => Action::InputBackspace,
                KeyCode::Char(c) if !ctrl => Action::InputChar(c),
                _ => Action::None,
            };
        }

        if let Some(prefix) = self.pending.take()
            && let KeyCode::Char(c) = key.code
        {
            return match (prefix, c) {
                (']', 'c') => Action::NextHunk,
                ('[', 'c') => Action::PrevHunk,
                (']', 'f') => Action::NextFile,
                ('[', 'f') => Action::PrevFile,
                _ => Action::None,
            };
        }

        if *overlay != Overlay::None {
            return match key.code {
                KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('?') => Action::Escape,
                _ => Action::None,
            };
        }

        match key.code {
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Esc => return Action::Escape,
            KeyCode::Char('?') => return Action::ToggleHelp,
            KeyCode::Char('m') => return Action::ToggleMode,
            KeyCode::Char('t') => return Action::SetMode(Mode::Tree),
            KeyCode::Char('d') if !ctrl => return Action::SetMode(Mode::Diff),
            KeyCode::Char('r') => return Action::Reload,
            // 表示形式のトグルはフォーカス位置に関係なく効かせる
            KeyCode::Char('u') if !ctrl => return Action::ToggleUnified,
            KeyCode::Char('w') if !ctrl => return Action::ToggleWrap,
            // 比較対象の切り替えは tree モードのステータス記号にも効く
            KeyCode::Char('s') if !ctrl => return Action::CycleDiffSpec,
            KeyCode::Char('R') => return Action::StartRange,
            KeyCode::Tab => return Action::CycleFocus(1),
            KeyCode::BackTab => return Action::CycleFocus(-1),
            KeyCode::Char('<') => return Action::ResizeTree(-1),
            KeyCode::Char('>') => return Action::ResizeTree(1),
            KeyCode::Char(c @ (']' | '[')) => {
                self.pending = Some(c);
                return Action::None;
            }
            _ => {}
        }

        match focus {
            Focus::Tree => Self::map_tree(key, ctrl),
            Focus::Content => Self::map_content(key, ctrl),
        }
    }

    fn map_tree(key: KeyEvent, ctrl: bool) -> Action {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => Action::TreeMove(1),
            KeyCode::Char('k') | KeyCode::Up => Action::TreeMove(-1),
            KeyCode::Char('d') if ctrl => Action::TreeHalfPage(1),
            KeyCode::Char('u') if ctrl => Action::TreeHalfPage(-1),
            KeyCode::Char('n') if ctrl => Action::NextFile,
            KeyCode::Char('p') if ctrl => Action::PrevFile,
            KeyCode::Char('g') => Action::TreeFirst,
            KeyCode::Char('G') => Action::TreeLast,
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => Action::TreeOpen,
            KeyCode::Char('h') | KeyCode::Left => Action::TreeCollapse,
            KeyCode::Char('z') => Action::TreeToggle,
            KeyCode::Char('I') => Action::ToggleIgnored,
            KeyCode::Char('T') => Action::ToggleHierarchy,
            KeyCode::Char('S') => Action::CycleSort,
            KeyCode::Char('a') => Action::Stage,
            KeyCode::Char('U') => Action::Unstage,
            KeyCode::Char('f') | KeyCode::Char('/') => Action::StartFilter,
            _ => Action::None,
        }
    }

    fn map_content(key: KeyEvent, ctrl: bool) -> Action {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => Action::ContentScroll(1),
            KeyCode::Char('k') | KeyCode::Up => Action::ContentScroll(-1),
            KeyCode::Char('d') if ctrl => Action::ContentHalfPage(1),
            KeyCode::Char('u') if ctrl => Action::ContentHalfPage(-1),
            KeyCode::Char('f') if ctrl => Action::ContentPage(1),
            KeyCode::Char('b') if ctrl => Action::ContentPage(-1),
            KeyCode::Char('n') if ctrl => Action::NextHunk,
            KeyCode::Char('p') if ctrl => Action::PrevHunk,
            KeyCode::Char('g') => Action::ContentFirst,
            KeyCode::Char('G') => Action::ContentLast,
            KeyCode::Char('h') | KeyCode::Left => Action::ContentHScroll(-4),
            KeyCode::Char('l') | KeyCode::Right => Action::ContentHScroll(4),
            KeyCode::Char('z') => Action::ToggleFold,
            KeyCode::Enter => Action::ExpandGap,
            KeyCode::Char('/') => Action::StartSearch,
            KeyCode::Char('n') => Action::NextMatch,
            KeyCode::Char('N') => Action::PrevMatch,
            _ => Action::None,
        }
    }
}
