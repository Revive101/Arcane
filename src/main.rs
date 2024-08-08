use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use fuzzywuzzy::fuzz::ratio;
use parser::parser::{Asset, AssetFetcher};
use ratatui::{
    backend::CrosstermBackend,
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::Line,
    widgets::{Block, BorderType, Borders, HighlightSpacing, List, ListItem, ListState, StatefulWidget, Widget},
    Terminal,
};
use revision_checker::Revision;
use std::{
    cmp::Reverse,
    collections::VecDeque,
    io::{self, Write},
};
use tui_textarea::TextArea;

pub mod errors;
mod parser;
mod revision_checker;
pub mod util;

const VERSION: &str = "1.0.1";

/// This struct holds the current state of the app.
struct App {
    pub assets: AssetList,
    asset_fetcher: AssetFetcher,
    layout: Layout,
    inner_layout: Layout,
    inner_layout_extended_info: Layout,
    extended_info: bool,
}

#[derive(Clone)]
struct AssetList {
    pub state: ListState,
    pub items: Vec<Asset>,
    pub filtered_items: Vec<Asset>,
    pub last_selected: Option<usize>,
    selected_assets: Option<Asset>,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut terminal = init_terminal()?;

    // TODO: Move to own function??
    let revision = Revision::check().await.unwrap();
    let mut asset_fetcher = AssetFetcher::new(revision.clone());
    asset_fetcher.load_index().await;

    App::new(AssetList::with_items(asset_fetcher.clone().assets), asset_fetcher).run(&mut terminal)?;

    restore_terminal(&mut terminal)?;

    Ok(())
}

fn init_terminal() -> io::Result<Terminal<CrosstermBackend<io::StdoutLock<'static>>>> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    enable_raw_mode()?;
    crossterm::execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;

    Ok(terminal)
}
fn restore_terminal<W: Write>(terminal: &mut Terminal<CrosstermBackend<W>>) -> io::Result<()> {
    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()
}

impl App {
    fn new(assets: AssetList, asset_fetcher: AssetFetcher) -> Self {
        let layout = Layout::default().constraints([Constraint::Min(1), Constraint::Length(3)].as_slice());
        let inner_layout = Layout::new(Direction::Horizontal, [Constraint::Percentage(100)]);
        let inner_layout_extended_info = Layout::new(Direction::Horizontal, [Constraint::Percentage(75), Constraint::Percentage(25)]);

        Self {
            assets,
            layout,
            inner_layout,
            asset_fetcher,
            extended_info: false,
            inner_layout_extended_info,
        }
    }

    /// Changes the status of the selected `ListItem`
    fn change_status(&mut self) {
        if let Some(nth) = self.assets.state.selected() {
            if let Some(asset) = self.assets.filtered_items.get_mut(nth) {
                asset.already_fetched = true;
                self.asset_fetcher.fetch_asset(asset);

                // ngughh
                if let Some(item) = self.assets.items.iter_mut().find(|a| a.filename == asset.filename) {
                    item.already_fetched = true;
                }
            }
        }
    }

    fn toggle_info(&mut self) {
        if let Some(_) = self.assets.state.selected() {
            self.extended_info = !self.extended_info;
        } else {
            self.extended_info = false;
        }
    }

    fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<io::StdoutLock<'static>>>) -> io::Result<()> {
        let mut textarea = TextArea::default();
        textarea.set_cursor_line_style(Style::default());
        textarea.set_placeholder_text("Enter a filename");
        textarea.set_block(Block::new().border_type(BorderType::Rounded).borders(Borders::all()));
        let layout = Layout::default().constraints([Constraint::Min(1), Constraint::Length(3)].as_slice());

        loop {
            self.draw(terminal, &layout, &mut textarea)?;

            if let Event::Key(key) = event::read()? {
                match key {
                    KeyEvent { code: KeyCode::Esc, .. } => break,
                    KeyEvent { code: KeyCode::Enter, .. } => self.change_status(),
                    KeyEvent { code: KeyCode::Up, .. } => self.assets.previous(),
                    KeyEvent { code: KeyCode::Down, .. } => self.assets.next(),
                    KeyEvent {
                        code: KeyCode::Char(' '), ..
                    } => self.toggle_info(),
                    _ => {
                        textarea.input(key);

                        let content = &textarea.lines()[0].to_string();
                        self.assets.filter_and_sort(&content);
                    }
                }
            }
        }

        Ok(())
    }

    /// Called on every tick
    fn draw(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::StdoutLock<'static>>>,
        layout: &Layout,
        textarea: &mut TextArea,
    ) -> io::Result<()> {
        terminal.draw(|f| {
            f.render_widget(self, f.size());

            let chunks = layout.split(f.size());
            f.render_widget(textarea.widget(), chunks[1]);
        })?;

        Ok(())
    }
}

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let chunks = self.layout.split(area);

        let block = Block::new()
            .border_type(BorderType::Rounded)
            .borders(Borders::all())
            .title(Line::from(format!(" Arcane (Asset-Fetcher) v{VERSION} ")).centered())
            .title(Line::from(format!(" {} ", self.asset_fetcher.revision)).left_aligned())
            .title(Line::from(format!(" {} assets found ", self.assets.asset_len())).right_aligned())
            .title_bottom(Line::from(" © Phill030 (Revive101) ").left_aligned())
            .title_bottom(Line::from(" Press [ESC] to abort ").right_aligned());

        block.render(chunks[0], buf);

        // List items
        let items: Vec<ListItem> = self
            .assets
            .filtered_items
            .iter()
            .enumerate()
            .map(|(i, asset)| asset.to_list_item(i))
            .collect();

        let list_block = match self.extended_info {
            true => Block::new()
                .border_type(BorderType::Rounded)
                .borders(Borders::all())
                .title("Assets"),
            false => Block::new().borders(Borders::NONE),
        };

        let list = List::new(items)
            .block(list_block)
            .style(Style::default().fg(Color::White))
            .highlight_style(Style::default().cyan())
            .highlight_symbol(">> ")
            .highlight_spacing(HighlightSpacing::Always);

        let inner = match self.extended_info {
            true => <Layout as Clone>::clone(&self.inner_layout_extended_info)
                .margin(1)
                .split(chunks[0]),
            false => <Layout as Clone>::clone(&self.inner_layout).margin(1).split(chunks[0]),
        };
        StatefulWidget::render(list, inner[0], buf, &mut self.assets.state);

        if self.extended_info {
            if let Some(selected_assets) = &self.assets.selected_assets {
                let info_items = vec![
                    ListItem::new(format!("Name: {}", selected_assets.filename)),
                    ListItem::new(format!("Size: {}", bytes_to_human_readable(selected_assets.size))),
                    ListItem::new(format!("CRC: {}", selected_assets.crc)),
                    ListItem::new(format!("HeaderCRC: {}", selected_assets.header_crc)),
                    ListItem::new(format!("HeaderSize: {}", selected_assets.header_size)),
                    ListItem::new(format!("CompressedHeaderSize: {}", selected_assets.compressed_header_size)),
                ];

                let extended_info_list = List::new(info_items).block(
                    Block::new()
                        .border_type(BorderType::Rounded)
                        .borders(Borders::all())
                        .title(" Details ")
                        .style(Style::default().fg(Color::White)),
                );

                Widget::render(extended_info_list, inner[1], buf);
            }
        }
    }
}

impl AssetList {
    fn with_items(items: VecDeque<Asset>) -> Self {
        let mut items = Vec::from(items);
        items.sort();

        Self {
            state: ListState::default(),
            filtered_items: items.clone(),
            items,
            last_selected: None,
            selected_assets: None,
        }
    }

    fn next(&mut self) {
        match self.state.selected() {
            Some(nth) => {
                let to_select = if nth >= self.filtered_items.len() - 1 { 0 } else { nth + 1 };
                self.state.select(Some(to_select));

                match self.filtered_items.get(to_select) {
                    Some(asset) => self.selected_assets = Some(asset.clone()),
                    None => self.selected_assets = None,
                }
            }
            None => {
                self.state.select(Some(self.last_selected.unwrap_or(0)));
                self.selected_assets = None;
            }
        }
    }

    fn previous(&mut self) {
        match self.state.selected() {
            Some(nth) => {
                let to_select = if nth == 0 { self.filtered_items.len() - 1 } else { nth - 1 };
                self.state.select(Some(to_select));

                match self.filtered_items.get(to_select) {
                    Some(asset) => self.selected_assets = Some(asset.clone()),
                    None => self.selected_assets = None,
                }
            }
            None => {
                self.state.select(Some(self.last_selected.unwrap_or(0)));
                self.selected_assets = None;
            }
        }
    }

    fn asset_len(&self) -> usize {
        self.items.len()
    }

    fn filter_and_sort(&mut self, query: &str) {
        if query.is_empty() {
            self.filtered_items = self.items.clone();
            return;
        }

        self.filtered_items = self
            .items
            .iter()
            .filter(|asset| ratio(query, &asset.filename) >= 18)
            .cloned()
            .collect();

        self.filtered_items
            .sort_by_cached_key(|asset| Reverse(ratio(query, &asset.filename)));
    }
}

impl Asset {
    fn to_list_item(&self, _index: usize) -> ListItem {
        let line = match self.already_fetched {
            true => format!("{} [{}]  ✔", self.filename, bytes_to_human_readable(self.size)),
            false => format!("{} [{}] ", self.filename, bytes_to_human_readable(self.size)),
        };

        ListItem::new(line)
    }
}

fn bytes_to_human_readable(bytes: i64) -> String {
    const KILOBYTE: f64 = 1024.0;
    const MEGABYTE: f64 = KILOBYTE * 1024.0;
    const GIGABYTE: f64 = MEGABYTE * 1024.0;

    let bytes = bytes as f64;
    if bytes < KILOBYTE {
        format!("{} B", bytes)
    } else if bytes < MEGABYTE {
        format!("{:.1} KB", bytes / KILOBYTE)
    } else if bytes < GIGABYTE {
        format!("{:.1} MB", bytes / MEGABYTE)
    } else {
        format!("{:.1} GB", bytes / GIGABYTE)
    }
}
