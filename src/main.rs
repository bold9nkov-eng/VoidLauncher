use eframe::{egui, App};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, process::Command};

#[derive(Serialize, Deserialize, Clone)]
struct Settings { minecraft_dir: String, java_path: String, ram_gb: u32, version: String }
impl Default for Settings {
    fn default() -> Self {
        let base = if let Ok(p)=std::env::var("APPDATA") { PathBuf::from(p).join(".minecraft") }
        else if let Ok(h)=std::env::var("HOME") { PathBuf::from(h).join(".minecraft") } else { PathBuf::from(".minecraft") };
        Self { minecraft_dir: base.to_string_lossy().to_string(), java_path: "java".into(), ram_gb: 4, version: "1.21.11".into() }
    }
}
struct VoidLauncher { settings: Settings, status: String, tab: usize }
impl VoidLauncher {
    fn new() -> Self {
        let settings = fs::read_to_string("voidlauncher.json").ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Self { settings, status: "Готов к работе. Настрой папку игры.".into(), tab: 0 }
    }
    fn dir(&self) -> PathBuf { PathBuf::from(&self.settings.minecraft_dir) }
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
        let url="https://meta.fabricmc.net/v2/versions/installer";
        let dest=self.dir().join("voidlauncher-fabric-installer.jar");
        let download=if cfg!(target_os="windows") {
            Command::new("powershell").args(["-NoProfile","-Command",&format!("Invoke-WebRequest -Uri '{}' -OutFile '{}'",url,dest.to_string_lossy())]).status()
        } else { Command::new("curl").args(["-L",url,"-o",&dest.to_string_lossy()]).status() };
        match download {
            Ok(s) if s.success() => {
                let result=Command::new(&self.settings.java_path).arg("-jar").arg(&dest).arg("client").arg("-dir").arg(self.dir())
                    .arg("-mcversion").arg(&self.settings.version).arg("-loader").arg("latest").status();
                self.status=match result { Ok(s) if s.success()=>"Fabric installer завершил работу. Проверь профиль в официальном лаунчере.".into(),
                    Ok(s)=>format!("Установщик Fabric завершился с кодом {s}."), Err(e)=>format!("Не удалось запустить Java: {e}") };
            }
            Ok(s)=>self.status=format!("Не удалось скачать установщик Fabric: {s}"),
            Err(e)=>self.status=format!("Ошибка загрузки Fabric: {e}")
        }
    }
    fn launch_official(&mut self) {
        let result=if cfg!(target_os="windows") { Command::new("cmd").args(["/C","start","","minecraft://"]).spawn() }
        else if cfg!(target_os="macos") { Command::new("open").arg("minecraft://").spawn() }
        else { Command::new("xdg-open").arg("minecraft://").spawn() };
        self.status=match result { Ok(_)=>"Открываю официальный Minecraft Launcher. Выбери установленный Fabric-профиль.".into(),
            Err(e)=>format!("Не удалось открыть Minecraft Launcher: {e}") };
    }
}
impl App for VoidLauncher {
    fn update(&mut self, ctx:&egui::Context, _frame:&mut eframe::Frame) {
        egui::TopBottomPanel::top("header").show(ctx,|ui| { ui.horizontal(|ui| {
            ui.heading(egui::RichText::new("VOID").color(egui::Color32::from_rgb(190,110,255)).strong());
            ui.heading("LAUNCHER");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui| { ui.label("Fabric Edition"); });
        }); });
        egui::SidePanel::left("navigation").resizable(false).default_width(145.0).show(ctx,|ui| {
            ui.add_space(12.0);
            for (i,label) in ["Главная","Моды","Ресурспаки","Шейдеры","Настройки"].iter().enumerate() {
                if ui.selectable_label(self.tab==i,*label).clicked(){self.tab=i;}
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min),|ui| {ui.label(egui::RichText::new("VoidLauncher 0.1.0").weak());});
        });
        egui::CentralPanel::default().show(ctx,|ui| {
            ui.add_space(8.0);
            match self.tab {
                0=>{
                    ui.heading("Добро пожаловать в VoidLauncher");
                    ui.label("Лёгкий лаунчер Minecraft с Fabric и управлением файлами.");
                    ui.add_space(16.0);
                    ui.group(|ui|{
                        ui.label("Версия Minecraft");
                        egui::ComboBox::from_id_salt("version").selected_text(&self.settings.version).show_ui(ui,|ui|{
                            ui.selectable_value(&mut self.settings.version,"1.21.11".into(),"1.21.11");
                            ui.selectable_value(&mut self.settings.version,"26.3".into(),"26.3 (если поддерживается Fabric)");
                        });
                        ui.label(format!("Папка игры: {}",self.settings.minecraft_dir));
                        if ui.button("Выбрать папку игры").clicked(){if let Some(p)=rfd::FileDialog::new().pick_folder(){self.settings.minecraft_dir=p.to_string_lossy().to_string();}}
                        if ui.button("Установить Fabric для выбранной версии").clicked(){self.install_fabric();}
                    });
                    ui.add_space(12.0);
                    if ui.button(egui::RichText::new("ОТКРЫТЬ MINECRAFT LAUNCHER").strong()).clicked(){self.launch_official();}
                    ui.label("После установки Fabric выбери профиль Fabric в официальном лаунчере.");
                    ui.hyperlink_to("Открыть сайт Ely.by для регистрации / входа","https://ely.by/");
                    ui.label("Вход Ely.by внутри VoidLauncher пока не реализован. Не вводи пароль Ely.by в приложение.");
                }
                1=>{
                    ui.heading("Моды Fabric");
                    ui.label("Импортируй .jar-файлы в папку mods.");
                    if ui.button("Добавить мод (.jar)").clicked(){self.import_file("mods",&["jar"]);}
                    if ui.button("Открыть папку mods").clicked(){self.open_dir("mods");}
                    ui.label("Для отключения переименуй .jar в .jar.disabled через проводник.");
                }
                2=>{
                    ui.heading("Ресурспаки");
                    if ui.button("Импортировать ZIP").clicked(){self.import_file("resourcepacks",&["zip"]);}
                    if ui.button("Открыть папку resourcepacks").clicked(){self.open_dir("resourcepacks");}
                    ui.label("Активируй набор в настройках Minecraft.");
                }
                3=>{
                    ui.heading("Шейдеры");
                    ui.label("Для шейдеров установи Iris в Fabric-профиль.");
                    if ui.button("Импортировать шейдерпак ZIP").clicked(){self.import_file("shaderpacks",&["zip"]);}
                    if ui.button("Открыть папку shaderpacks").clicked(){self.open_dir("shaderpacks");}
                }
                _=>{
                    ui.heading("Настройки");
                    ui.horizontal(|ui|{ui.label("Оперативная память (ГБ):");ui.add(egui::DragValue::new(&mut self.settings.ram_gb).range(2..=32));});
                    ui.horizontal(|ui|{ui.label("Путь к Java:");ui.text_edit_singleline(&mut self.settings.java_path);});
                    ui.label("Для Minecraft 1.21.11 обычно нужна Java 21.");
                    if ui.button("Сохранить настройки").clicked(){self.save();}
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
