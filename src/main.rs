use eframe::{egui, App};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, process::Command, sync::mpsc::{self, Receiver}};
#[cfg(windows)] use std::os::windows::ffi::OsStrExt;

#[derive(Serialize, Deserialize, Clone)]
struct Settings { minecraft_dir: String, java_path: String, ram_gb: u32, version: String, #[serde(default = "default_username")] username: String, #[serde(default)] instances: Vec<Instance>, #[serde(default)] active_instance: usize }
fn default_username() -> String { "Player".into() }
#[derive(Serialize, Deserialize, Clone)]
struct Instance { name: String, version: String, path: String }
impl Default for Settings {
    fn default() -> Self {
        let base = if let Ok(p)=std::env::var("APPDATA") { PathBuf::from(p).join(".minecraft") }
        else if let Ok(h)=std::env::var("HOME") { PathBuf::from(h).join(".minecraft") } else { PathBuf::from(".minecraft") };
        Self { minecraft_dir: base.to_string_lossy().to_string(), java_path: "auto".into(), ram_gb: 4, version: "1.21.11".into(), username: "Player".into(), instances: Vec::new(), active_instance: 0 }
    }
}
#[derive(Serialize, Deserialize, Clone)]
struct ElySession { username: String, uuid: String, access_token: String }
#[derive(Deserialize)]
struct ElyProfile { id: String, name: String }
#[derive(Deserialize)]
struct ElyAuthResponse {
    #[serde(rename = "accessToken")] access_token: Option<String>,
    #[serde(rename = "selectedProfile")] selected_profile: Option<ElyProfile>,
    #[serde(rename = "errorMessage")] error_message: Option<String>,
    error: Option<String>,
}
struct VoidLauncher {
    settings: Settings, status: String, tab: usize, new_instance_name: String,
    receiver: Option<Receiver<String>>, busy: bool,
    ely_username: String, ely_password: String, ely_totp: String,
    ely_session: Option<ElySession>
}
fn java_major_for_version(version: &str) -> u32 {
    let parts: Vec<u32> = version.split('.').filter_map(|p| p.parse().ok()).collect();
    if parts.first() == Some(&1) {
        match parts.get(1).copied().unwrap_or(21) {
            21.. => 21,
            18..=20 => 17,
            17 => 16,
            _ => 8,
        }
    } else { 21 }
}

fn java_version_ok(executable: &std::path::Path, required: u32) -> bool {
    let Ok(output) = Command::new(executable).arg("-version").output() else { return false; };
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let Some(start) = text.find("version \"") else { return false; };
    let rest = &text[start + 9..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let major = if digits.starts_with("1.") { digits.split('.').nth(1).and_then(|x| x.parse::<u32>().ok()).unwrap_or(0) }
        else { digits.split('.').next().and_then(|x| x.parse::<u32>().ok()).unwrap_or(0) };
    major >= required
}

fn find_java_exe(root: &std::path::Path, depth: usize) -> Option<PathBuf> {
    if depth > 5 { return None; }
    for entry in fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_file() && path.file_name().and_then(|n| n.to_str()).map(|n| n.eq_ignore_ascii_case("java.exe")).unwrap_or(false) { return Some(path); }
        if path.is_dir() { if let Some(found) = find_java_exe(&path, depth + 1) { return Some(found); } }
    }
    None
}

fn ensure_java(configured: &str, version: &str, send: &impl Fn(String)) -> Result<PathBuf, String> {
    let required = java_major_for_version(version);
    let configured = configured.trim();
    if !configured.is_empty() && !configured.eq_ignore_ascii_case("auto") {
        let candidate = PathBuf::from(configured);
        if java_version_ok(&candidate, required) { return Ok(candidate); }
        if configured.eq_ignore_ascii_case("java") {
            let candidate = PathBuf::from("java");
            if java_version_ok(&candidate, required) { return Ok(candidate); }
        } else { return Err(format!("Указанная Java не подходит. Для Minecraft {version} нужна Java {required} или новее.")); }
    } else {
        let candidate = PathBuf::from("java");
        if java_version_ok(&candidate, required) { return Ok(candidate); }
    }
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).or_else(|| dirs::data_local_dir()).ok_or("Не удалось определить папку AppData для Java")?;
    let root = local.join("VoidLauncher").join("runtime").join(format!("java{required}"));
    if let Some(found) = find_java_exe(&root, 0) { if java_version_ok(&found, required) { return Ok(found); } }
    send(format!("Java {required} не найдена. Скачиваю Eclipse Temurin…"));
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let url = format!("https://api.adoptium.net/v3/binary/latest/{required}/ga/windows/x64/jre/hotspot/normal/eclipse");
    let client = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(900)).user_agent("VoidLauncher/1.0").build().map_err(|e| e.to_string())?;
    let mut bytes = None;
    let mut last_error = String::new();
    for attempt in 1..=4 {
        match client.get(&url).send().and_then(|r| r.error_for_status()).and_then(|r| r.bytes()) {
            Ok(data) => { bytes = Some(data); break; }
            Err(e) => { last_error = e.to_string(); if attempt < 4 { send(format!("Ошибка загрузки Java, повтор {attempt}/4…")); std::thread::sleep(std::time::Duration::from_secs(2_u64.pow(attempt as u32))); } }
        }
    }
    let bytes = bytes.ok_or_else(|| format!("Не удалось скачать Java: {last_error}"))?;
    send("Распаковываю Java…".into());
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut item = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(relative) = item.enclosed_name().map(|p| p.to_path_buf()) else { continue; };
        let target = root.join(relative);
        if item.is_dir() { fs::create_dir_all(&target).map_err(|e| e.to_string())?; }
        else {
            if let Some(parent) = target.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
            let mut out = fs::File::create(&target).map_err(|e| e.to_string())?;
            std::io::copy(&mut item, &mut out).map_err(|e| e.to_string())?;
        }
    }
    let java = find_java_exe(&root, 0).ok_or("После распаковки Java не найден java.exe")?;
    if !java_version_ok(&java, required) { return Err("Загруженная Java не прошла проверку версии".into()); }
    Ok(java)
}

impl VoidLauncher {
    fn new() -> Self {
        let settings = fs::read_to_string("voidlauncher.json").ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Self { settings, status: "Создай свой первый экземпляр Minecraft.".into(), tab: 0, new_instance_name: "Vanilla 1.21.11".into(), receiver: None, busy: false, ely_username: String::new(), ely_password: String::new(), ely_totp: String::new(), ely_session: None }
    }
    fn dir(&self) -> PathBuf { self.settings.instances.get(self.settings.active_instance).map(|i|PathBuf::from(&i.path)).unwrap_or_else(||PathBuf::from(&self.settings.minecraft_dir).join("instances").join("Default")) }
    fn create_instance(&mut self) {
        let name=self.new_instance_name.trim().to_string();
        if name.is_empty() { self.status="Введи название экземпляра.".into(); return; }
        let safe=name.chars().map(|c|if c.is_ascii_alphanumeric() || c==' ' || c=='-' || c=='_' {c} else {'_'}).collect::<String>();
        let path=PathBuf::from(&self.settings.minecraft_dir).join("instances").join(&safe);
        if path.exists() { self.status="Экземпляр с таким именем уже существует.".into(); return; }
        if let Err(e)=fs::create_dir_all(&path) { self.status=format!("Не удалось создать экземпляр: {e}"); return; }
        for folder in ["mods","resourcepacks","shaderpacks","config"] { let _=fs::create_dir_all(path.join(folder)); }
        self.settings.instances.push(Instance{name,version:self.settings.version.clone(),path:path.to_string_lossy().to_string()});
        self.settings.active_instance=self.settings.instances.len()-1;
        self.save(); self.status="Экземпляр создан. Папки mods, resourcepacks и shaderpacks готовы.".into();
    }
    fn open_instance_folder(&mut self, folder:&str) { let p=self.dir().join(folder); let _=fs::create_dir_all(&p); self.open_path(&p); }
    fn open_path(&mut self,p:&std::path::Path) {
        let result=if cfg!(target_os="windows") { Command::new("explorer").arg(p).spawn() } else if cfg!(target_os="macos") { Command::new("open").arg(p).spawn() } else { Command::new("xdg-open").arg(p).spawn() };
        self.status=match result {Ok(_)=>format!("Открыта папка: {}",p.display()),Err(e)=>format!("Не удалось открыть папку: {e}")};
    }
    fn save(&mut self) {
        self.status = match serde_json::to_string_pretty(&self.settings).ok().and_then(|s| fs::write("voidlauncher.json",s).ok()) {
            Some(_) => "Настройки сохранены.".into(), None => "Не удалось сохранить настройки.".into()
        };
    }
    fn open_dir(&mut self, name: &str) {
        let p=self.dir().join(name);
        if let Err(e)=fs::create_dir_all(&p) { self.status=format!("Ошибка папки: {e}"); return; }
        let result=if cfg!(target_os="windows") { Command::new("explorer").arg(&p).spawn() }
        else if cfg!(target_os="macos") { Command::new("open").arg(&p).spawn() }
        else { Command::new("xdg-open").arg(&p).spawn() };
        self.status=match result { Ok(_) => format!("Открыта папка {name}"), Err(e)=>format!("Не удалось открыть папку: {e}") };
    }
    fn import_file(&mut self, folder: &str, extensions: &[&str]) {
        if let Some(file)=rfd::FileDialog::new().add_filter("Поддерживаемые файлы",extensions).pick_file() {
            if let Some(name)=file.file_name() {
                let dest=self.dir().join(folder);
                self.status=match fs::create_dir_all(&dest).and_then(|_| fs::copy(&file,dest.join(name)).map(|_|())) {
                    Ok(_) => format!("Файл добавлен в {folder}."), Err(e)=>format!("Ошибка импорта: {e}")
                };
            }
        }
    }
    fn install_fabric(&mut self) {
        self.status = "Fabric устанавливается автоматически при нажатии «Играть».".into();
    }
    fn toggle_file(&mut self, folder: &str, file_name: &str) {
        let path=self.dir().join(folder).join(file_name);
        let new_name=if file_name.ends_with(".disabled") { file_name.trim_end_matches(".disabled").to_string() }
            else { format!("{file_name}.disabled") };
        self.status=match fs::rename(&path,path.with_file_name(new_name)) {
            Ok(())=>"Состояние файла изменено.".into(), Err(e)=>format!("Не удалось изменить файл: {e}")
        };
    }
    fn remove_file(&mut self, folder: &str, file_name: &str) {
        let path=self.dir().join(folder).join(file_name);
        self.status=match fs::remove_file(path) { Ok(())=>"Файл удалён.".into(), Err(e)=>format!("Не удалось удалить файл: {e}") };
    }
    fn show_file_list(&mut self, ui: &mut egui::Ui, folder: &str, title: &str) {
        ui.separator();
        ui.label(egui::RichText::new(title).strong());
        let dir=self.dir().join(folder);
        let _=fs::create_dir_all(&dir);
        let mut files:Vec<String>=fs::read_dir(&dir).ok().into_iter().flatten()
            .filter_map(|e|e.ok()).filter(|e|e.path().is_file())
            .filter_map(|e|e.file_name().into_string().ok()).collect();
        files.sort();
        if files.is_empty() { ui.label("Пока файлов нет."); }
        for name in files {
            let disabled=name.ends_with(".disabled");
            ui.horizontal(|ui| {
                ui.label(if disabled {"⏸"} else {"●"});
                ui.label(&name);
                if ui.small_button(if disabled {"Включить"} else {"Отключить"}).clicked() { self.toggle_file(folder,&name); }
                if ui.small_button("Удалить").clicked() { self.remove_file(folder,&name); }
            });
        }
    }
    fn login_ely(&mut self) {
        let username = self.ely_username.trim().to_string();
        let mut password = self.ely_password.clone();
        let totp = self.ely_totp.trim().to_string();
        if username.is_empty() || password.is_empty() {
            self.status = "Укажи логин/email и пароль Ely.by.".into();
            return;
        }
        if !totp.is_empty() { password.push(':'); password.push_str(&totp); }
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        self.busy = true;
        self.status = "Выполняется вход в Ely.by…".into();
        std::thread::spawn(move || {
            let result = (|| -> Result<ElySession, String> {
                let client_token = uuid::Uuid::new_v4().to_string();
                let client = reqwest::blocking::Client::builder()
                    .timeout(std::time::Duration::from_secs(25)).build().map_err(|e| e.to_string())?;
                let response = client.post("https://authserver.ely.by/auth/authenticate")
                    .json(&serde_json::json!({
                        "username": username, "password": password,
                        "clientToken": client_token, "requestUser": true
                    })).send().map_err(|e| format!("Не удалось связаться с Ely.by: {e}"))?;
                let status = response.status();
                let body: ElyAuthResponse = response.json().map_err(|e| format!("Некорректный ответ Ely.by: {e}"))?;
                if !status.is_success() {
                    let message = body.error_message.unwrap_or_else(|| body.error.unwrap_or_else(|| format!("HTTP {status}")));
                    if message.to_lowercase().contains("two factor") {
                        return Err("Для аккаунта включена 2FA. Введи одноразовый код в поле TOTP и нажми «Войти» ещё раз.".into());
                    }
                    return Err(format!("Ely.by: {message}"));
                }
                let profile = body.selected_profile.ok_or("Ely.by не вернул выбранный игровой профиль")?;
                let token = body.access_token.ok_or("Ely.by не вернул accessToken")?;
                Ok(ElySession { username: profile.name, uuid: profile.id, access_token: token })
            })();
            match result {
                Ok(session) => {
                    let payload = serde_json::to_string(&session).unwrap_or_default();
                    let _ = tx.send(format!("ELY_OK:{payload}"));
                }
                Err(error) => { let _ = tx.send(format!("Ошибка входа: {error}")); }
            }
        });
    }

    fn launch_official(&mut self) {
        let Some(instance) = self.settings.instances.get(self.settings.active_instance).cloned() else {
            self.status = "Сначала создай экземпляр.".into(); return;
        };
        let username = self.settings.username.trim().to_string();
        let ely_session = self.ely_session.clone();
        let use_ely = ely_session.is_some();
        if ely_session.is_none() && (username.is_empty() || username.len() > 16 || !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')) {
            self.status = "Укажи игровой ник или войди через Ely.by.".into(); return;
        }
        let java = self.settings.java_path.clone();
        let version = instance.version.clone();
        let game_dir = PathBuf::from(&instance.path);
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        self.busy = true;
        self.status = format!("Установка Minecraft {version} + Fabric…");
        std::thread::spawn(move || {
            let send = |message: String| { let _ = tx.send(message); };
            let result = (|| -> std::result::Result<(), String> {
                fs::create_dir_all(&game_dir).map_err(|e| e.to_string())?;
                let java_executable = ensure_java(&java, &version, &send)?;
                send(format!("Скачивание файлов Minecraft {version} и Fabric…"));
                let launcher = mc_launcher_core::prelude::Launcher::new(&game_dir);
                let mut installed = None;
                let mut last_error = String::from("Не удалось установить Minecraft.");
                for attempt in 1..=5 {
                    let request = mc_launcher_core::prelude::InstallRequest {
                        minecraft_version: version.clone(),
                        loader: Some(mc_launcher_core::prelude::LoaderSpec::Fabric {
                            version: mc_launcher_core::prelude::LoaderVersion::LatestStable,
                        }),
                        java: mc_launcher_core::prelude::JavaInstallPolicy::Auto,
                    };
                    match launcher.install(request) {
                        Ok(value) => { installed = Some(value); break; }
                        Err(error) => {
                            last_error = error.to_string();
                            let lower = last_error.to_lowercase();
                            let retryable = lower.contains("network")
                                || lower.contains("error sending request")
                                || lower.contains("timed out")
                                || lower.contains("timeout")
                                || lower.contains("connection")
                                || lower.contains("temporarily unavailable")
                                || lower.contains("tls");
                            if !retryable || attempt == 5 { break; }
                            let delay = 2_u64.pow(attempt as u32);
                            send(format!("Сеть прервала загрузку. Повторная попытка {}/5 через {} сек…", attempt + 1, delay));
                            std::thread::sleep(std::time::Duration::from_secs(delay));
                        }
                    }
                }
                let installed = installed.ok_or_else(|| format!(
                    "{} После 5 попыток загрузка не завершилась. Проверь соединение и нажми «Играть» ещё раз — уже скачанные файлы будут проверены повторно.",
                    last_error
                ))?;
                send("Игра установлена. Подготовка Java-команды…".into());
                let profile = launcher.load_version(&installed.version_id).map_err(|e| e.to_string())?;
                let ely_session_for_args = ely_session.clone();
                let account = if let Some(session) = ely_session {
                    mc_launcher_core::prelude::Account::Microsoft {
                        username: session.username,
                        uuid: session.uuid,
                        access_token: session.access_token,
                    }
                } else {
                    mc_launcher_core::prelude::Account::offline(username)
                };
                let options = mc_launcher_core::prelude::LaunchOptions {
                    account,
                    java_executable: Some(java_executable),
                    game_directory: Some(game_dir.clone()),
                    ..Default::default()
                };
                let launch = launcher.build_launch_command_from_version(&profile, options).map_err(|e| e.to_string())?;
                let mut command = Command::new(&launch.executable);
                if use_ely {
                    let injector = game_dir.join("authlib-injector-1.2.8.jar");
                    if !injector.exists() {
                        send("Загрузка компонента авторизации Ely.by…".into());
                        let bytes = reqwest::blocking::get("https://github.com/yushijinhun/authlib-injector/releases/download/v1.2.8/authlib-injector-1.2.8.jar")
                            .and_then(|r| r.error_for_status()).map_err(|e| format!("Не удалось скачать authlib-injector: {e}"))?
                            .bytes().map_err(|e| e.to_string())?;
                        fs::write(&injector, &bytes).map_err(|e| e.to_string())?;
                    }
                    command.arg(format!("-javaagent:{}=ely.by", injector.display()));
                }
                let mut launch_args: Vec<std::ffi::OsString> = launch.args.into_iter().map(Into::into).collect();
                if let Some(session) = &ely_session_for_args {
                    let replacements = [
                        ("--username", session.username.as_str()),
                        ("--uuid", session.uuid.as_str()),
                        ("--accessToken", session.access_token.as_str()),
                        ("--userType", "mojang"),
                        ("--userProperties", "{}"),
                    ];
                    let mut index = 0;
                    while index + 1 < launch_args.len() {
                        let key = launch_args[index].to_string_lossy().to_string();
                        if let Some((_, value)) = replacements.iter().find(|(flag, _)| *flag == key) {
                            launch_args[index + 1] = (*value).into();
                            index += 2;
                        } else { index += 1; }
                    }
                }
                command.args(&launch_args).current_dir(&launch.working_dir);
                for (key, value) in launch.env { command.env(key, value); }
                #[cfg(windows)] {
                    use std::os::windows::process::CommandExt;
                    command.creation_flags(0x08000000);
                }
                command.spawn().map_err(|e| format!("Не удалось запустить Minecraft: {e}"))?;
                Ok(())
            })();
            match result {
                Ok(()) => send("Готово: Minecraft запущен.".into()),
                Err(error) => send(format!("Ошибка: {error}")),
            }
        });
    }
}
impl App for VoidLauncher {
    fn update(&mut self, ctx:&egui::Context, _frame:&mut eframe::Frame) {
        if let Some(rx) = &self.receiver {
            while let Ok(message) = rx.try_recv() {
                if let Some(payload) = message.strip_prefix("ELY_OK:") {
                    match serde_json::from_str::<ElySession>(payload) {
                        Ok(session) => {
                            self.status = format!("Вход выполнен: {} (Ely.by).", session.username);
                            self.ely_session = Some(session);
                            self.ely_password.clear();
                            self.ely_totp.clear();
                            self.busy = false;
                        }
                        Err(e) => { self.status = format!("Ошибка обработки сессии Ely.by: {e}"); self.busy = false; }
                    }
                } else {
                    self.status = message;
                    if self.status.starts_with("Готово:") || self.status.starts_with("Ошибка") { self.busy = false; }
                }
            }
        }
        let mut visuals=egui::Visuals::dark();
        visuals.panel_fill=egui::Color32::from_rgb(18,12,31);
        visuals.window_fill=egui::Color32::from_rgb(24,15,42);
        visuals.extreme_bg_color=egui::Color32::from_rgb(10,7,18);
        visuals.widgets.noninteractive.bg_fill=egui::Color32::from_rgb(31,20,52);
        visuals.widgets.inactive.bg_fill=egui::Color32::from_rgb(52,29,82);
        visuals.widgets.hovered.bg_fill=egui::Color32::from_rgb(100,48,150);
        visuals.widgets.active.bg_fill=egui::Color32::from_rgb(142,72,205);
        ctx.set_visuals(visuals);
        egui::TopBottomPanel::top("header").frame(egui::Frame::default().fill(egui::Color32::from_rgb(35,17,58)).inner_margin(12.0)).show(ctx,|ui| { ui.horizontal(|ui| {
            ui.heading(egui::RichText::new("VOID").color(egui::Color32::from_rgb(190,110,255)).strong());
            ui.heading("LAUNCHER");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui| { ui.label("Fabric Edition"); });
        }); });
        egui::SidePanel::left("navigation").resizable(false).default_width(145.0).show(ctx,|ui| {
            ui.add_space(12.0);
            for (i,label) in ["Экземпляры","Настройки","Аккаунт Ely.by"].iter().enumerate() {
                if ui.selectable_label(self.tab==i,*label).clicked(){self.tab=i;}
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min),|ui| {ui.label(egui::RichText::new("VoidLauncher 0.1.0").weak());});
        });
        egui::CentralPanel::default().show(ctx,|ui| {
            ui.add_space(8.0);
            match self.tab {
                0=>{
                    ui.heading(egui::RichText::new("ТВОИ ЭКЗЕМПЛЯРЫ").color(egui::Color32::from_rgb(205,145,255)).size(26.0).strong());
                    ui.label("Отдельные сборки Minecraft — у каждой свои моды, ресурспаки и настройки.");
                    ui.add_space(14.0);
                    egui::Frame::group(ui.style()).fill(egui::Color32::from_rgb(34,20,55)).rounding(12.0).inner_margin(14.0).show(ui,|ui|{
                        ui.label(egui::RichText::new("НОВЫЙ ЭКЗЕМПЛЯР").color(egui::Color32::from_rgb(190,125,245)).strong());
                        ui.horizontal(|ui|{ui.text_edit_singleline(&mut self.new_instance_name); if ui.button("＋ Создать").clicked(){self.create_instance();}});
                    });
                    ui.add_space(12.0);
                    if self.settings.instances.is_empty(){ui.label("Пока нет экземпляров. Создай первый выше.");}
                    let instances=self.settings.instances.clone();
                    for (i,instance) in instances.iter().enumerate(){
                        egui::Frame::group(ui.style()).fill(if i==self.settings.active_instance {egui::Color32::from_rgb(55,29,83)} else {egui::Color32::from_rgb(27,18,43)}).rounding(10.0).inner_margin(12.0).show(ui,|ui|{
                            ui.horizontal(|ui|{
                                ui.vertical(|ui|{
                                    ui.label(egui::RichText::new(&instance.name).size(18.0).strong());
                                    ui.label(format!("Minecraft {} • Fabric",instance.version));
                                    ui.label(egui::RichText::new(&instance.path).weak().small());
                                });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                                    if ui.add_enabled(!self.busy, egui::Button::new("▶ Играть")).clicked(){self.settings.active_instance=i;self.launch_official();}
                                    if ui.button("Папка").clicked(){self.settings.active_instance=i;self.open_path(std::path::Path::new(&instance.path));}
                                    if ui.button("Выбрать").clicked(){self.settings.active_instance=i;}
                                });
                            });
                        });
                        ui.add_space(6.0);
                    }
                }
                1=>{
                    ui.heading(egui::RichText::new("НАСТРОЙКИ").color(egui::Color32::from_rgb(205,145,255)).size(25.0).strong());
                    if let Some(inst)=self.settings.instances.get(self.settings.active_instance).cloned(){
                        ui.label(format!("Активный экземпляр: {} ({})",inst.name,inst.version));
                        ui.label("Папки экземпляра — добавляй файлы напрямую:");
                        ui.horizontal_wrapped(|ui|{
                            if ui.button("📁 mods").clicked(){self.open_instance_folder("mods");}
                            if ui.button("📁 resourcepacks").clicked(){self.open_instance_folder("resourcepacks");}
                            if ui.button("📁 shaderpacks").clicked(){self.open_instance_folder("shaderpacks");}
                            if ui.button("📁 config").clicked(){self.open_instance_folder("config");}
                            if ui.button("📂 Папка экземпляра").clicked(){let p=self.dir();self.open_path(&p);}
                        });
                        ui.separator();
                    } else {ui.label("Сначала создай экземпляр на вкладке «Экземпляры».");}
                    const MC_VERSIONS: &[&str] = &[
                        "1.21.11", "1.21.10", "1.21.9", "1.21.8", "1.21.7",
                        "1.21.6", "1.21.5", "1.21.4", "1.21.3", "1.21.1",
                        "1.20.6", "1.20.4", "1.20.1", "1.19.4", "1.18.2",
                        "1.16.5", "1.12.2"
                    ];
                    ui.horizontal(|ui| {
                        ui.label("Версия Minecraft:");
                        egui::ComboBox::from_id_salt("minecraft_version_select")
                            .selected_text(&self.settings.version)
                            .show_ui(ui, |ui| {
                                for version in MC_VERSIONS {
                                    ui.selectable_value(&mut self.settings.version, (*version).to_string(), *version);
                                }
                            });
                    });
                    ui.label("Выбранная версия применяется к новым экземплярам.");
                    if let Some(index) = self.settings.instances.get(self.settings.active_instance).map(|_| self.settings.active_instance) {
                        let current = self.settings.instances[index].version.clone();
                        if ui.button(format!("Применить {} к активному экземпляру (сейчас {})", self.settings.version, current)).clicked() {
                            self.settings.instances[index].version = self.settings.version.clone();
                            self.save();
                            self.status = "Версия экземпляра обновлена. Нажми «Играть», чтобы установить её.".into();
                        }
                    }
                    ui.horizontal(|ui|{ui.label("ОЗУ (ГБ):");ui.add(egui::DragValue::new(&mut self.settings.ram_gb).range(2..=32));});
                    ui.horizontal(|ui|{ui.label("Игровой ник (Offline):");ui.text_edit_singleline(&mut self.settings.username);});
                    ui.horizontal(|ui|{ui.label("Java (auto):");ui.text_edit_singleline(&mut self.settings.java_path);});
                    ui.label("Java: auto — лаунчер сам найдёт или скачает подходящую версию.");
                    ui.horizontal(|ui|{ui.label("Корневая папка:");ui.text_edit_singleline(&mut self.settings.minecraft_dir);});
                    if ui.button("Сохранить настройки").clicked(){self.save();}
                    ui.hyperlink_to("Регистрация и аккаунт Ely.by","https://ely.by/");
                }
                _=>{
                    ui.heading(egui::RichText::new("АККАУНТ ELY.BY").color(egui::Color32::from_rgb(205,145,255)).size(25.0).strong());
                    if let Some(session) = &self.ely_session {
                        ui.label(format!("Выполнен вход: {}", session.username));
                        if ui.button("Выйти из Ely.by").clicked() { self.ely_session = None; self.status = "Выход из аккаунта выполнен.".into(); }
                    } else {
                        ui.label("Войди через Ely.by, чтобы запускать Minecraft с этим аккаунтом.");
                        ui.label("Логин или email:");
                        ui.text_edit_singleline(&mut self.ely_username);
                        ui.label("Пароль:");
                        ui.add(egui::TextEdit::singleline(&mut self.ely_password).password(true));
                        ui.label("TOTP-код (только если включена 2FA):");
                        ui.text_edit_singleline(&mut self.ely_totp);
                        if ui.add_enabled(!self.busy, egui::Button::new("Войти через Ely.by")).clicked() { self.login_ely(); }
                        ui.hyperlink_to("Открыть Ely.by","https://ely.by/");
                        ui.label(egui::RichText::new("Пароль не сохраняется; сессия действует только пока открыт лаунчер.").weak().small());
                    }
                    ui.separator();
                    ui.label("Offline-вход остаётся доступен во вкладке «Настройки».");
                }
            }
            ui.add_space(20.0);ui.separator();
            ui.label(egui::RichText::new(format!("Статус: {}",self.status)).color(egui::Color32::from_rgb(190,160,220)));
        });
    }
}
fn main()->eframe::Result<()> {
    let options=eframe::NativeOptions{viewport:egui::ViewportBuilder::default().with_inner_size([900.0,620.0]).with_min_inner_size([720.0,500.0]),..Default::default()};
    eframe::run_native("VoidLauncher",options,Box::new(|_cc|Ok(Box::new(VoidLauncher::new()))))
}

#[cfg(windows)]
fn open_minecraft_launcher() -> Result<(), String> {
    use std::{ffi::OsStr, ptr};
    let verb: Vec<u16> = OsStr::new("open").encode_wide().chain(std::iter::once(0)).collect();
    let target: Vec<u16> = OsStr::new("minecraft://").encode_wide().chain(std::iter::once(0)).collect();
    let result = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            ptr::null_mut(), verb.as_ptr(), target.as_ptr(),
            ptr::null(), ptr::null(), 1
        )
    };
    if (result as isize) > 32 {
        Ok(())
    } else {
        Err(format!("Windows не смог открыть ссылку minecraft:// (код {}). Проверь установку официального Minecraft Launcher.", result as isize))
    }
}

#[cfg(not(windows))]
fn open_minecraft_launcher() -> Result<(), String> {
    open::that("minecraft://").map_err(|e| e.to_string())
}
