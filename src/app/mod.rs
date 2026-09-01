pub mod action;
pub mod state;
pub mod update;

pub use action::KeyMap;
pub use state::*;

use std::sync::mpsc::Receiver;

use anyhow::Result;
use ratatui::crossterm::event::Event;
use ratatui::{Terminal, backend::Backend};

use crate::task::AppEvent;

pub fn run<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    rx: &Receiver<AppEvent>,
) -> Result<()>
where
    B::Error: std::error::Error + Send + Sync + 'static,
{
    let mut keymap = KeyMap::default();
    loop {
        terminal.draw(|frame| crate::ui::draw(frame, app))?;
        if app.should_quit {
            return Ok(());
        }
        // 単一チャネルをブロッキング受信するため、待機中の CPU 使用率は 0 になる
        let Ok(event) = rx.recv() else {
            return Ok(());
        };
        handle(app, &mut keymap, event);
        // 溜まっている分は描画を挟まずまとめて処理する
        while !app.should_quit {
            match rx.try_recv() {
                Ok(event) => handle(app, &mut keymap, event),
                Err(_) => break,
            }
        }
    }
}

fn handle(app: &mut App, keymap: &mut KeyMap, event: AppEvent) {
    match event {
        AppEvent::Input(Event::Key(key)) => {
            let action = keymap.map(key, app.focus, &app.overlay);
            update::apply(app, action);
        }
        AppEvent::Input(_) => {}
        AppEvent::Task(result) => update::on_task(app, result),
        AppEvent::FsChanged => update::on_fs_change(app),
    }
}
