#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod tray;

use iced::widget::{
    button, checkbox, column, container, pick_list, row, rule, scrollable, text, text_input,
};
use iced::{
    Alignment, Color, Element, Fill, Length, Size, Subscription, Task, Theme, event, time, window,
};
use portweave::config::{AppConfig, ForwardKind, TunnelConfig, config_path};
use portweave::ssh::{ProcessEvent, TunnelManager};
use portweave::ssh_config::{SshConfigImport, SshConnection, import_default_ssh_config};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;
use uuid::Uuid;

const APP_NAME: &str = "PortWeave";
const MAX_LOG_LINES: usize = 500;

fn main() -> iced::Result {
    let (tray_handle, tray_error) = match tray::create() {
        Ok(handle) => (Some(handle), None),
        Err(error) => (None, Some(error)),
    };
    let tray_available = tray_handle.is_some();
    let _tray = tray_handle;
    iced::application(
        move || PortWeave::boot(tray_available, tray_error.clone()),
        PortWeave::update,
        PortWeave::view,
    )
    .title(APP_NAME)
    .theme(PortWeave::theme)
    .subscription(PortWeave::subscription)
    .window_size(Size::new(1120.0, 720.0))
    .centered()
    .exit_on_close_request(false)
    .run()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Tunnels,
    Logs,
    Settings,
    Editor,
    Import,
}

#[derive(Debug, Clone)]
enum Message {
    Navigate(Page),
    Tick,
    WindowEvent(window::Id, window::Event),
    Start(Uuid),
    Stop(Uuid),
    StartAll,
    StopAll,
    Edit(Uuid),
    RequestDelete(Uuid),
    ConfirmDelete(Uuid),
    CancelDelete,
    NewTunnel,
    ImportSshConfig,
    SshConfigImported(Result<SshConfigImport, String>),
    SelectImportedConnection(usize),
    CancelImport,
    SaveTunnel,
    CancelEdit,
    DismissBanner,
    NameChanged(String),
    HostChanged(String),
    UserChanged(String),
    SshPortChanged(String),
    KindChanged(ForwardKind),
    BindAddressChanged(String),
    BindPortChanged(String),
    TargetHostChanged(String),
    TargetPortChanged(String),
    IdentityFileChanged(String),
    ProxyJumpChanged(String),
    AutostartChanged(bool),
    MinimizeToTrayChanged(bool),
    StartMinimizedChanged(bool),
    Quit,
}

struct PortWeave {
    config: AppConfig,
    config_path: PathBuf,
    page: Page,
    form: TunnelForm,
    manager: TunnelManager,
    event_tx: Sender<ProcessEvent>,
    event_rx: Receiver<ProcessEvent>,
    logs: VecDeque<String>,
    errors: HashMap<Uuid, String>,
    banner: Option<String>,
    window_id: Option<window::Id>,
    initial_window_event_seen: bool,
    tray_available: bool,
    pending_delete: Option<Uuid>,
    imported_connections: Vec<SshConnection>,
    import_source: Option<PathBuf>,
    importing: bool,
}

impl PortWeave {
    fn boot(tray_available: bool, tray_error: Option<String>) -> (Self, Task<Message>) {
        let (event_tx, event_rx) = mpsc::channel();
        let (config, config_path, config_error) = match AppConfig::load() {
            Ok((config, path)) => (config, path, None),
            Err(error) => (
                AppConfig::default(),
                config_path().unwrap_or_else(|_| PathBuf::from("portweave-config.json")),
                Some(format!("Could not load the saved configuration: {error}")),
            ),
        };
        let banner = config_error.or_else(|| {
            tray_error.map(|error| {
                format!("System tray is unavailable; closing the window will exit: {error}")
            })
        });
        let mut app = Self {
            config,
            config_path,
            page: Page::Tunnels,
            form: TunnelForm::default(),
            manager: TunnelManager::default(),
            event_tx,
            event_rx,
            logs: VecDeque::new(),
            errors: HashMap::new(),
            banner,
            window_id: None,
            initial_window_event_seen: false,
            tray_available,
            pending_delete: None,
            imported_connections: Vec::new(),
            import_source: None,
            importing: false,
        };

        let autostart_ids: Vec<_> = app
            .config
            .tunnels
            .iter()
            .filter(|tunnel| tunnel.autostart)
            .map(|tunnel| tunnel.id)
            .collect();
        for id in autostart_ids {
            app.start_tunnel(id);
        }
        (app, Task::none())
    }

    fn theme(&self) -> Theme {
        Theme::TokyoNight
    }

    fn subscription(&self) -> Subscription<Message> {
        let ticks = time::every(Duration::from_millis(250)).map(|_| Message::Tick);
        let window_events = event::listen_with(|event, _status, id| match event {
            iced::Event::Window(event) => Some(Message::WindowEvent(id, event)),
            _ => None,
        });
        Subscription::batch([ticks, window_events])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Navigate(page) => self.page = page,
            Message::Tick => return self.on_tick(),
            Message::WindowEvent(id, event) => {
                self.window_id = Some(id);
                if !self.initial_window_event_seen {
                    self.initial_window_event_seen = true;
                    if self.config.start_minimized {
                        return window::set_mode(id, window::Mode::Hidden);
                    }
                }
                if matches!(event, window::Event::CloseRequested) {
                    if self.config.minimize_to_tray && self.tray_available {
                        self.push_log("Window hidden. PortWeave is still running in the tray.");
                        return window::set_mode(id, window::Mode::Hidden);
                    }
                    return self.quit();
                }
            }
            Message::Start(id) => self.start_tunnel(id),
            Message::Stop(id) => self.stop_tunnel(id),
            Message::StartAll => {
                let ids: Vec<_> = self.config.tunnels.iter().map(|tunnel| tunnel.id).collect();
                for id in ids {
                    self.start_tunnel(id);
                }
            }
            Message::StopAll => {
                let ids: Vec<_> = self.config.tunnels.iter().map(|tunnel| tunnel.id).collect();
                for id in ids {
                    self.stop_tunnel(id);
                }
            }
            Message::Edit(id) => {
                if let Some(tunnel) = self.config.tunnels.iter().find(|item| item.id == id) {
                    self.form = TunnelForm::from_tunnel(tunnel);
                    self.page = Page::Editor;
                }
            }
            Message::RequestDelete(id) => self.pending_delete = Some(id),
            Message::ConfirmDelete(id) => {
                self.stop_tunnel(id);
                let name = self
                    .config
                    .tunnels
                    .iter()
                    .find(|tunnel| tunnel.id == id)
                    .map(|tunnel| tunnel.name.clone())
                    .unwrap_or_else(|| "Tunnel".into());
                self.config.tunnels.retain(|tunnel| tunnel.id != id);
                self.errors.remove(&id);
                self.save_config();
                self.push_log(format!("Deleted “{name}”."));
                self.pending_delete = None;
            }
            Message::CancelDelete => self.pending_delete = None,
            Message::NewTunnel => {
                self.form = TunnelForm::default();
                self.page = Page::Editor;
            }
            Message::ImportSshConfig => {
                self.importing = true;
                return Task::perform(
                    async { import_default_ssh_config() },
                    Message::SshConfigImported,
                );
            }
            Message::SshConfigImported(result) => {
                self.importing = false;
                match result {
                    Ok(import) => {
                        let count = import.connections.len();
                        self.import_source = Some(import.config_path);
                        self.imported_connections = import.connections;
                        self.page = Page::Import;
                        if import.warnings.is_empty() {
                            self.push_log(format!(
                                "Found {count} connection(s) in the OpenSSH config."
                            ));
                        } else {
                            let skipped = import.warnings.len();
                            self.banner = Some(format!(
                                "Found {count} connection(s); {skipped} Host alias(es) could not be resolved."
                            ));
                            for warning in import.warnings {
                                self.push_log(format!("[SSH import] Skipped {warning}"));
                            }
                        }
                    }
                    Err(error) => {
                        self.banner = Some(format!("Could not import SSH config: {error}"))
                    }
                }
            }
            Message::SelectImportedConnection(index) => {
                if let Some(connection) = self.imported_connections.get(index) {
                    self.form = TunnelForm::from_ssh_connection(connection);
                    self.page = Page::Editor;
                }
            }
            Message::CancelImport => self.page = Page::Tunnels,
            Message::SaveTunnel => self.save_tunnel(),
            Message::CancelEdit => {
                self.form = TunnelForm::default();
                self.page = Page::Tunnels;
            }
            Message::DismissBanner => self.banner = None,
            Message::NameChanged(value) => self.form.name = value,
            Message::HostChanged(value) => self.form.host = value,
            Message::UserChanged(value) => self.form.user = value,
            Message::SshPortChanged(value) => self.form.ssh_port = digits_only(value),
            Message::KindChanged(value) => self.form.kind = value,
            Message::BindAddressChanged(value) => self.form.bind_address = value,
            Message::BindPortChanged(value) => self.form.bind_port = digits_only(value),
            Message::TargetHostChanged(value) => self.form.target_host = value,
            Message::TargetPortChanged(value) => self.form.target_port = digits_only(value),
            Message::IdentityFileChanged(value) => self.form.identity_file = value,
            Message::ProxyJumpChanged(value) => self.form.proxy_jump = value,
            Message::AutostartChanged(value) => self.form.autostart = value,
            Message::MinimizeToTrayChanged(value) => {
                self.config.minimize_to_tray = value;
                self.save_config();
            }
            Message::StartMinimizedChanged(value) => {
                self.config.start_minimized = value;
                self.save_config();
            }
            Message::Quit => return self.quit(),
        }
        Task::none()
    }

    fn on_tick(&mut self) -> Task<Message> {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                ProcessEvent::Output { id, line } => {
                    let name = self.tunnel_name(id);
                    self.push_log(format!("[{name}] {line}"));
                }
            }
        }

        for (id, result) in self.manager.poll_exited() {
            let name = self.tunnel_name(id);
            match result {
                Ok(code) => {
                    let message = format!("SSH exited with code {code}");
                    self.errors.insert(id, message.clone());
                    self.push_log(format!("[{name}] {message}."));
                }
                Err(error) => {
                    self.errors.insert(id, error.clone());
                    self.push_log(format!("[{name}] Could not inspect SSH process: {error}"));
                }
            }
        }

        match tray::poll() {
            Some(tray::TrayAction::Show) => self.show_window(),
            Some(tray::TrayAction::Quit) => self.quit(),
            None => Task::none(),
        }
    }

    fn start_tunnel(&mut self, id: Uuid) {
        let Some(config) = self.config.tunnels.iter().find(|tunnel| tunnel.id == id) else {
            return;
        };
        let name = config.name.clone();
        match self.manager.start(config, self.event_tx.clone()) {
            Ok(pid) => {
                self.errors.remove(&id);
                self.push_log(format!("[{name}] Started SSH process {pid}."));
            }
            Err(error) => {
                self.errors.insert(id, error.clone());
                self.banner = Some(format!("Could not start “{name}”: {error}"));
                self.push_log(format!("[{name}] Start failed: {error}"));
            }
        }
    }

    fn stop_tunnel(&mut self, id: Uuid) {
        let name = self.tunnel_name(id);
        match self.manager.stop(id) {
            Ok(true) => self.push_log(format!("[{name}] Stopped.")),
            Ok(false) => {}
            Err(error) => {
                self.banner = Some(error.clone());
                self.push_log(format!("[{name}] Stop failed: {error}"));
            }
        }
    }

    fn save_tunnel(&mut self) {
        match self.form.to_tunnel() {
            Ok(tunnel) => {
                let id = tunnel.id;
                if self.manager.is_running(id) {
                    self.stop_tunnel(id);
                    self.push_log(format!(
                        "[{}] Stopped because its configuration changed.",
                        tunnel.name
                    ));
                }
                if let Some(existing) = self
                    .config
                    .tunnels
                    .iter_mut()
                    .find(|item| item.id == tunnel.id)
                {
                    *existing = tunnel;
                } else {
                    self.config.tunnels.push(tunnel);
                }
                self.save_config();
                self.form = TunnelForm::default();
                self.errors.remove(&id);
                self.page = Page::Tunnels;
            }
            Err(error) => self.form.error = Some(error),
        }
    }

    fn save_config(&mut self) {
        if let Err(error) = self.config.save(&self.config_path) {
            self.banner = Some(format!("Could not save configuration: {error}"));
        }
    }

    fn push_log(&mut self, line: impl Into<String>) {
        if self.logs.len() == MAX_LOG_LINES {
            self.logs.pop_front();
        }
        self.logs.push_back(line.into());
    }

    fn tunnel_name(&self, id: Uuid) -> String {
        self.config
            .tunnels
            .iter()
            .find(|tunnel| tunnel.id == id)
            .map(|tunnel| tunnel.name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    fn show_window(&self) -> Task<Message> {
        self.window_id.map_or_else(Task::none, |id| {
            Task::batch([
                window::set_mode(id, window::Mode::Windowed),
                window::gain_focus(id),
            ])
        })
    }

    fn quit(&mut self) -> Task<Message> {
        self.manager.stop_all();
        iced::exit()
    }

    fn view(&self) -> Element<'_, Message> {
        let sidebar = self.sidebar();
        let body = match self.page {
            Page::Tunnels => self.tunnels_page(),
            Page::Logs => self.logs_page(),
            Page::Settings => self.settings_page(),
            Page::Editor => self.editor_page(),
            Page::Import => self.import_page(),
        };

        let mut content = column![body].width(Fill).height(Fill);
        if let Some(message) = self.banner.as_deref() {
            let banner = container(
                row![
                    text(message).size(13).width(Fill),
                    button("Dismiss")
                        .style(button::text)
                        .on_press(Message::DismissBanner)
                ]
                .align_y(Alignment::Center),
            )
            .style(container::dark)
            .padding([10, 16]);
            content = column![banner, content];
        }

        row![sidebar, content].width(Fill).height(Fill).into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let brand = column![
            text("PORTWEAVE").size(20),
            text("SSH tunnel manager")
                .size(12)
                .color(Color::from_rgb8(148, 163, 184))
        ]
        .spacing(3);

        let navigation = column![
            nav_button("Tunnels", Page::Tunnels, self.page),
            nav_button("Activity log", Page::Logs, self.page),
            nav_button("Settings", Page::Settings, self.page),
        ]
        .spacing(8);

        let status = column![
            text(format!("{} active", self.manager.running_count()))
                .size(13)
                .color(Color::from_rgb8(74, 222, 128)),
            text(format!("{} configured", self.config.tunnels.len()))
                .size(12)
                .color(Color::from_rgb8(148, 163, 184)),
        ]
        .spacing(4);

        container(
            column![
                brand,
                rule::horizontal(1),
                navigation,
                container(status).height(Fill)
            ]
            .spacing(22)
            .height(Fill),
        )
        .width(Length::Fixed(224.0))
        .height(Fill)
        .padding(24)
        .style(container::dark)
        .into()
    }

    fn page_header<'a>(&self, title: &'a str, subtitle: &'a str) -> Element<'a, Message> {
        column![
            text(title).size(30),
            text(subtitle)
                .size(14)
                .color(Color::from_rgb8(148, 163, 184))
        ]
        .spacing(5)
        .into()
    }

    fn tunnels_page(&self) -> Element<'_, Message> {
        let controls = row![
            self.page_header(
                "SSH tunnels",
                "Secure forwards managed by your system OpenSSH"
            ),
            row![
                button("Start all")
                    .style(button::secondary)
                    .on_press(Message::StartAll),
                button("Stop all")
                    .style(button::secondary)
                    .on_press(Message::StopAll),
                if self.importing {
                    button("Importing SSH config…").style(button::secondary)
                } else {
                    button("Import SSH config")
                        .style(button::secondary)
                        .on_press(Message::ImportSshConfig)
                },
                button("+ New tunnel")
                    .style(button::primary)
                    .on_press(Message::NewTunnel),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
        ]
        .align_y(Alignment::Center)
        .spacing(20);

        let list: Element<'_, Message> = if self.config.tunnels.is_empty() {
            container(
                column![
                    text("No tunnels yet").size(22),
                    text("Create a remote or local port forward in a few fields.")
                        .color(Color::from_rgb8(148, 163, 184)),
                    button("Create the first tunnel")
                        .style(button::primary)
                        .on_press(Message::NewTunnel)
                ]
                .spacing(12)
                .align_x(Alignment::Center),
            )
            .center_x(Fill)
            .center_y(Fill)
            .height(Fill)
            .style(container::rounded_box)
            .into()
        } else {
            let mut cards = column![].spacing(12);
            for tunnel in &self.config.tunnels {
                cards = cards.push(self.tunnel_card(tunnel));
            }
            scrollable(cards).height(Fill).into()
        };

        container(column![controls, list].spacing(24))
            .padding(30)
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn tunnel_card<'a>(&'a self, tunnel: &'a TunnelConfig) -> Element<'a, Message> {
        let running = self.manager.is_running(tunnel.id);
        let (status, status_color) = if running {
            (
                format!(
                    "RUNNING · PID {}",
                    self.manager.pid(tunnel.id).unwrap_or_default()
                ),
                Color::from_rgb8(74, 222, 128),
            )
        } else if self.errors.contains_key(&tunnel.id) {
            ("NEEDS ATTENTION".into(), Color::from_rgb8(248, 113, 113))
        } else {
            ("STOPPED".into(), Color::from_rgb8(148, 163, 184))
        };

        let action = if running {
            button("Stop")
                .style(button::danger)
                .on_press(Message::Stop(tunnel.id))
        } else {
            button("Start")
                .style(button::primary)
                .on_press(Message::Start(tunnel.id))
        };

        let mut actions = row![action].spacing(10).align_y(Alignment::Center);
        if self.pending_delete == Some(tunnel.id) {
            actions = actions
                .push(
                    button("Confirm delete")
                        .style(button::danger)
                        .on_press(Message::ConfirmDelete(tunnel.id)),
                )
                .push(
                    button("Cancel")
                        .style(button::text)
                        .on_press(Message::CancelDelete),
                );
        } else {
            actions = actions
                .push(
                    button("Edit")
                        .style(button::secondary)
                        .on_press(Message::Edit(tunnel.id)),
                )
                .push(
                    button("Delete")
                        .style(button::text)
                        .on_press(Message::RequestDelete(tunnel.id)),
                );
        }

        let top = row![
            column![
                text(&tunnel.name).size(19),
                text(status).size(11).color(status_color)
            ]
            .spacing(5)
            .width(Fill),
            actions,
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let destination = tunnel.destination();
        let mapping = tunnel.mapping();
        let jump = tunnel.proxy_jump.as_deref().unwrap_or("Direct");
        let details = row![
            detail("SERVER", destination),
            detail("VIA", jump),
            detail("MODE", tunnel.kind.label()),
            detail("MAPPING", mapping),
            detail("AUTO-START", if tunnel.autostart { "On" } else { "Off" }),
        ]
        .spacing(30);

        let mut body = column![top, rule::horizontal(1), details].spacing(14);
        if let Some(error) = self.errors.get(&tunnel.id) {
            body = body.push(text(error).size(12).color(Color::from_rgb8(248, 113, 113)));
        }

        container(body)
            .padding(18)
            .width(Fill)
            .style(container::rounded_box)
            .into()
    }

    fn editor_page(&self) -> Element<'_, Message> {
        let title = if self.form.editing.is_some() {
            "Edit tunnel"
        } else {
            "New tunnel"
        };
        let subtitle = match self.form.kind {
            ForwardKind::Remote => "Expose a local service on the remote SSH server",
            ForwardKind::Local => "Reach a remote service through a local listening port",
        };

        let connection = section(
            "SSH connection",
            column![
                field(
                    "Name",
                    text_input("e.g. example-remote", &self.form.name)
                        .on_input(Message::NameChanged)
                ),
                row![
                    field(
                        "Server",
                        text_input("203.0.113.10", &self.form.host).on_input(Message::HostChanged)
                    ),
                    field(
                        "User",
                        text_input("developer", &self.form.user).on_input(Message::UserChanged)
                    ),
                    field(
                        "SSH port",
                        text_input("2222", &self.form.ssh_port)
                            .on_input(Message::SshPortChanged)
                            .width(Length::Fixed(150.0))
                    ),
                ]
                .spacing(12),
                field(
                    "Identity file (optional)",
                    text_input("C:\\Users\\you\\.ssh\\id_ed25519", &self.form.identity_file,)
                        .on_input(Message::IdentityFileChanged)
                ),
                field(
                    "Jump hosts (optional)",
                    text_input(
                        "bastion or bastion,ops@edge.example:2222",
                        &self.form.proxy_jump,
                    )
                    .on_input(Message::ProxyJumpChanged)
                ),
                text("Imported Host aliases continue to use matching options from ~/.ssh/config. Jump chains use OpenSSH ProxyJump syntax.")
                    .size(12)
                    .color(Color::from_rgb8(148, 163, 184)),
            ]
            .spacing(14),
        );

        let mapping = section(
            "Port mapping",
            column![
                column![
                    text("Direction")
                        .size(12)
                        .color(Color::from_rgb8(148, 163, 184)),
                    pick_list(ForwardKind::ALL, Some(self.form.kind), Message::KindChanged)
                        .width(Fill)
                ]
                .spacing(6),
                row![
                    field(
                        "Bind address",
                        text_input("127.0.0.1", &self.form.bind_address)
                            .on_input(Message::BindAddressChanged)
                    ),
                    field(
                        "Bind port",
                        text_input("7897", &self.form.bind_port).on_input(Message::BindPortChanged)
                    ),
                    text("→").size(22),
                    field(
                        "Target host",
                        text_input("127.0.0.1", &self.form.target_host)
                            .on_input(Message::TargetHostChanged)
                    ),
                    field(
                        "Target port",
                        text_input("7897", &self.form.target_port)
                            .on_input(Message::TargetPortChanged)
                    ),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
                checkbox(self.form.autostart)
                    .label("Start this tunnel when PortWeave launches")
                    .on_toggle(Message::AutostartChanged),
            ]
            .spacing(14),
        );

        let mut form = column![self.page_header(title, subtitle), connection, mapping].spacing(18);
        if let Some(error) = self.form.error.as_deref() {
            form = form.push(text(error).size(13).color(Color::from_rgb8(248, 113, 113)));
        }
        form = form.push(
            row![
                button("Cancel")
                    .style(button::secondary)
                    .on_press(Message::CancelEdit),
                button("Save tunnel")
                    .style(button::primary)
                    .on_press(Message::SaveTunnel),
            ]
            .spacing(10),
        );

        scrollable(container(form).padding(30).max_width(900)).into()
    }

    fn import_page(&self) -> Element<'_, Message> {
        let source = self
            .import_source
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "~/.ssh/config".into());
        let header = row![
            self.page_header(
                "Import SSH connection",
                "Choose a concrete Host alias, then complete its port mapping"
            ),
            button("Cancel")
                .style(button::secondary)
                .on_press(Message::CancelImport),
        ]
        .spacing(20)
        .align_y(Alignment::Center);

        let mut connections = column![
            text(format!("Source: {source}"))
                .size(12)
                .color(Color::from_rgb8(148, 163, 184))
        ]
        .spacing(12);
        for (index, connection) in self.imported_connections.iter().enumerate() {
            let route = connection
                .proxy_jump
                .as_deref()
                .map(|jump| format!("Via {jump}"))
                .unwrap_or_else(|| "Direct connection".into());
            connections = connections.push(
                container(
                    row![
                        column![
                            text(&connection.alias).size(18),
                            text(connection.destination())
                                .size(13)
                                .color(Color::from_rgb8(148, 163, 184)),
                            text(route).size(12).color(Color::from_rgb8(148, 163, 184)),
                        ]
                        .spacing(5)
                        .width(Fill),
                        button("Use connection")
                            .style(button::primary)
                            .on_press(Message::SelectImportedConnection(index)),
                    ]
                    .align_y(Alignment::Center),
                )
                .padding(16)
                .width(Fill)
                .style(container::rounded_box),
            );
        }

        container(column![header, scrollable(connections).height(Fill)].spacing(24))
            .padding(30)
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn logs_page(&self) -> Element<'_, Message> {
        let mut lines = column![].spacing(8);
        if self.logs.is_empty() {
            lines = lines.push(
                text("Tunnel activity will appear here.").color(Color::from_rgb8(148, 163, 184)),
            );
        } else {
            for line in self.logs.iter().rev() {
                lines = lines.push(text(line).size(13));
            }
        }
        container(
            column![
                self.page_header("Activity log", "The latest 500 events from this session"),
                container(scrollable(lines).height(Fill))
                    .padding(18)
                    .height(Fill)
                    .width(Fill)
                    .style(container::rounded_box)
            ]
            .spacing(24),
        )
        .padding(30)
        .width(Fill)
        .height(Fill)
        .into()
    }

    fn settings_page(&self) -> Element<'_, Message> {
        let minimize_checkbox = checkbox(self.config.minimize_to_tray)
            .label("Keep tunnels running when the window is closed");
        let minimize_checkbox = if self.tray_available {
            minimize_checkbox.on_toggle(Message::MinimizeToTrayChanged)
        } else {
            minimize_checkbox
        };
        let start_hidden_checkbox =
            checkbox(self.config.start_minimized).label("Start with the window hidden");
        let start_hidden_checkbox = if self.tray_available {
            start_hidden_checkbox.on_toggle(Message::StartMinimizedChanged)
        } else {
            start_hidden_checkbox
        };

        let behavior = section(
            "App behavior",
            column![
                minimize_checkbox,
                start_hidden_checkbox,
                text(if self.tray_available {
                    "Tray integration is ready."
                } else {
                    "Tray integration is unavailable on this system."
                })
                .size(12)
                .color(Color::from_rgb8(148, 163, 184)),
            ]
            .spacing(14),
        );

        let storage = section(
            "Local data",
            column![
                text("Configuration file")
                    .size(12)
                    .color(Color::from_rgb8(148, 163, 184)),
                text(self.config_path.display().to_string()).size(13),
                text("Passwords and private keys are never stored. Authentication is delegated to OpenSSH, your key files, and ssh-agent.")
                    .size(13)
                    .color(Color::from_rgb8(148, 163, 184)),
            ]
            .spacing(8),
        );

        let about = section(
            "About",
            column![
                text(format!("PortWeave {}", env!("CARGO_PKG_VERSION"))).size(16),
                text("Iced 0.14 · tiny-skia renderer · system OpenSSH")
                    .size(13)
                    .color(Color::from_rgb8(148, 163, 184)),
                button("Quit PortWeave")
                    .style(button::danger)
                    .on_press(Message::Quit),
            ]
            .spacing(10),
        );

        container(
            column![
                self.page_header("Settings", "Lightweight by design, private by default"),
                behavior,
                storage,
                about
            ]
            .spacing(18),
        )
        .padding(30)
        .max_width(900)
        .into()
    }
}

#[derive(Debug, Clone)]
struct TunnelForm {
    editing: Option<Uuid>,
    name: String,
    host: String,
    user: String,
    ssh_port: String,
    kind: ForwardKind,
    bind_address: String,
    bind_port: String,
    target_host: String,
    target_port: String,
    identity_file: String,
    proxy_jump: String,
    autostart: bool,
    error: Option<String>,
}

impl Default for TunnelForm {
    fn default() -> Self {
        Self {
            editing: None,
            name: String::new(),
            host: String::new(),
            user: String::new(),
            ssh_port: "22".into(),
            kind: ForwardKind::Remote,
            bind_address: "127.0.0.1".into(),
            bind_port: String::new(),
            target_host: "127.0.0.1".into(),
            target_port: String::new(),
            identity_file: String::new(),
            proxy_jump: String::new(),
            autostart: false,
            error: None,
        }
    }
}

impl TunnelForm {
    fn from_tunnel(tunnel: &TunnelConfig) -> Self {
        Self {
            editing: Some(tunnel.id),
            name: tunnel.name.clone(),
            host: tunnel.host.clone(),
            user: tunnel.user.clone(),
            ssh_port: tunnel.ssh_port.to_string(),
            kind: tunnel.kind,
            bind_address: tunnel.bind_address.clone(),
            bind_port: tunnel.bind_port.to_string(),
            target_host: tunnel.target_host.clone(),
            target_port: tunnel.target_port.to_string(),
            identity_file: tunnel
                .identity_file
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            proxy_jump: tunnel.proxy_jump.clone().unwrap_or_default(),
            autostart: tunnel.autostart,
            error: None,
        }
    }

    fn from_ssh_connection(connection: &SshConnection) -> Self {
        Self {
            name: connection.alias.clone(),
            host: connection.alias.clone(),
            user: connection.user.clone(),
            ssh_port: connection.port.to_string(),
            proxy_jump: connection.proxy_jump.clone().unwrap_or_default(),
            ..Self::default()
        }
    }

    fn to_tunnel(&self) -> Result<TunnelConfig, String> {
        let parse_port = |label: &str, value: &str| {
            value
                .parse::<u16>()
                .ok()
                .filter(|port| *port > 0)
                .ok_or_else(|| format!("{label} must be between 1 and 65535"))
        };
        let identity_file = (!self.identity_file.trim().is_empty())
            .then(|| PathBuf::from(self.identity_file.trim()));
        let proxy_jump =
            (!self.proxy_jump.trim().is_empty()).then(|| self.proxy_jump.trim().to_string());
        let tunnel = TunnelConfig {
            id: self.editing.unwrap_or_else(Uuid::new_v4),
            name: self.name.trim().into(),
            host: self.host.trim().into(),
            user: self.user.trim().into(),
            ssh_port: parse_port("SSH port", &self.ssh_port)?,
            kind: self.kind,
            bind_address: self.bind_address.trim().into(),
            bind_port: parse_port("Bind port", &self.bind_port)?,
            target_host: self.target_host.trim().into(),
            target_port: parse_port("Target port", &self.target_port)?,
            identity_file,
            proxy_jump,
            autostart: self.autostart,
        };
        tunnel.validate()?;
        Ok(tunnel)
    }
}

fn digits_only(value: String) -> String {
    value.chars().filter(char::is_ascii_digit).collect()
}

fn nav_button(label: &str, page: Page, current: Page) -> Element<'_, Message> {
    let style = if page == current {
        button::primary
    } else {
        button::text
    };
    button(text(label).width(Fill))
        .width(Fill)
        .padding([10, 12])
        .style(style)
        .on_press(Message::Navigate(page))
        .into()
}

fn detail<'a>(
    label: &'a str,
    value: impl iced::widget::text::IntoFragment<'a>,
) -> Element<'a, Message> {
    column![
        text(label).size(10).color(Color::from_rgb8(148, 163, 184)),
        text(value).size(13)
    ]
    .spacing(4)
    .into()
}

fn section<'a>(title: &'a str, body: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(column![text(title).size(17), body.into()].spacing(14))
        .padding(18)
        .width(Fill)
        .style(container::rounded_box)
        .into()
}

fn field<'a>(label: &'a str, input: iced::widget::TextInput<'a, Message>) -> Element<'a, Message> {
    column![
        text(label).size(12).color(Color::from_rgb8(148, 163, 184)),
        input.padding(11).size(14)
    ]
    .spacing(6)
    .width(Fill)
    .into()
}
