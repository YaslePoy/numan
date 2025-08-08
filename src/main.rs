use clap::{Parser, Subcommand};
use dirs_next::config_dir;
use reqwest::{Client, multipart};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::fs;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write, stdout};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::str::FromStr;
use std::string::ToString;
use std::time::Duration;

/// Nuget packages manager
#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
struct CliArgs {
    /// Path to .nupkg file to send
    path: Option<String>,

    /// Personal nuget api key
    #[arg(short, long)]
    key: Option<String>,

    #[arg(short, long)]
    overlook: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug, Clone)]
enum Commands {
    /// Remember personal nuget api key
    Auth {
        key: String,
    },
    /// Forget personal nuget api key
    Logout,
    /// Show current configuration
    ShowCfg,
    /// Check newer packets versions
    Check,
    Publish,
}
#[derive(Serialize, Deserialize, Debug, Clone)]
struct Configuration {
    key: Option<String>,
    packets: Vec<Packet>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Packet {
    key: String,
    version: Version,
    path: String,
}
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone)]
struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    fn new(major: u32, minor: u32, patch: u32) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    fn from_name(name: &str) -> Version {
        let mut parts = name.split(".").collect::<Vec<&str>>();
        parts.pop();
        let c = u32::from_str(parts.pop().unwrap()).unwrap();
        let b = u32::from_str(parts.pop().unwrap()).unwrap();
        let a = u32::from_str(parts.pop().unwrap()).unwrap();
        Version::new(a, b, c)
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let ord = self.major.cmp(&other.major);
        if ord == Ordering::Equal {
            let ord = self.minor.cmp(&other.minor);
            if ord == Ordering::Equal {
                return self.patch.cmp(&other.patch);
            }
            return ord;
        }
        ord
    }
}

impl Packet {
    pub fn new(path: &Path) -> Packet {
        let name = path.file_name().unwrap().to_str().unwrap();
        let mut parts = name.split(".").collect::<Vec<&str>>();
        parts.pop();
        let c = u32::from_str(parts.pop().unwrap()).unwrap();
        let b = u32::from_str(parts.pop().unwrap()).unwrap();
        let a = u32::from_str(parts.pop().unwrap()).unwrap();

        let name = parts.join(".").to_string();

        let dir = path.parent().unwrap().to_str().unwrap();
        Packet {
            key: name,
            version: Version {
                major: a,
                minor: b,
                patch: c,
            },
            path: dir.to_string(),
        }
    }
}
fn config_path() -> PathBuf {
    config_dir()
        .expect("Could not determine config directory")
        .join("numan")
        .join("config.json")
}

fn read_config() -> Configuration {
    match File::open(config_path()) {
        Ok(file) => {
            let reader = BufReader::new(file);
            let config: Configuration = serde_json::from_reader(reader).unwrap();
            config
        }
        Err(_) => Configuration {
            key: None,
            packets: vec![],
        },
    }
}

fn write_config(config: Configuration) {
    let path = config_path();

    let file = File::create(&path).unwrap();

    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, &config).unwrap();
}

fn try_remember_packet(path: &Path, config: &mut Configuration) {
    match config
        .packets
        .iter_mut()
        .find(|p| p.path == path.parent().unwrap().to_str().unwrap())
    {
        Some(packet) => {
            let version = Version::from_name(&path.file_name().unwrap().to_str().unwrap());
            let more = packet.version < version;
            if more {
                packet.version = version;
            }
        }
        None => config.packets.push(Packet::new(path)),
    }

    write_config(config.clone());
}

fn conv(str: &mut String) {
    let last = str.pop().unwrap();
    str.insert(0, last);
}

async fn send_packet(path: &String, args: CliArgs) -> bool {
    let api_key: String;

    let config = &mut read_config();

    if let Some(key) = args.key {
        api_key = key;
    } else if let Some(key) = &config.key {
        api_key = (*key).clone();
    } else {
        println!("Api key is not defined! Use -k [key]");
        exit(4)
    }

    let form = multipart::Form::new().file("", path.clone()).await.unwrap();
    let req_thread = tokio::spawn(async {
        let client = Client::new();

        let response_process = client
            .put("https://www.nuget.org/api/v2/package/")
            .header("X-NuGet-ApiKey", api_key)
            .header("X-NuGet-Client-Version", "4.1.0")
            .multipart(form)
            .send();
        response_process.await
    });
    let mut ind = "---->---->---->".to_string();

    print!(
        "{} [{}] https://www.nuget.org/api/v2/package/{}",
        path,
        ind,
        "\x08".repeat(54)
    );
    while !req_thread.is_finished() {
        print!("{}", ind);
        stdout().flush().unwrap();
        print!("\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08");
        stdout().flush().unwrap();
        conv(&mut ind);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    println!();
    let response = req_thread.await.unwrap();
    match response {
        Ok(response) => {
            let status = response.status();
            println!("Response: [{}] {}", status, response.text().await.unwrap());
            if !status.is_success() {
                // return false;
            }
            if !args.overlook {
                try_remember_packet(&Path::new(&path), config);
            }
            true
        }
        Err(error) => {
            println!("Error: {:?}", error.status());
            false
        }
    }
}

#[tokio::main]
async fn main() {
    let args = CliArgs::parse();
    let args_clone = args.clone();
    let config_folder = config_dir()
        .expect("Could not determine config directory")
        .join("numan");

    if !Path::exists(&config_folder) {
        fs::create_dir_all(&config_folder).unwrap()
    }

    if let Some(_) = args.path {
        single_packet(args_clone).await;
        return;
    }

    match &args.command {
        Some(Commands::Auth { key }) => {
            regular_key(key);
        }

        Some(Commands::ShowCfg) => {
            show_config();
        }
        Some(Commands::Logout) => {
            logout();
        }
        Some(Commands::Check) => check(),
        Some(Commands::Publish) => {
            publish(&args_clone).await;
        }
        None => {}
    }
}

async fn publish(args: &CliArgs) {
    println!("Publishing newer versions");
    let config = read_config();
    for packet in config.packets {
        let path = packet.path;
        print!("{} ({}) Current: [{}]", packet.key, path, packet.version);
        let versions = find_packets(Path::new(&path));
        if let Some(newer) = versions.iter().find(|v| v.version > packet.version) {
            println!(" -> [{}]", newer.version);
            println!("{:?}", newer);
            send_packet(
                &format!("{}/{}.{}.nupkg", newer.path, newer.key, newer.version),
                args.clone(),
            )
            .await;
        } else {
            println!()
        }
    }
}

async fn single_packet(args: CliArgs) {
    let args_clone = args.clone();
    let path = args_clone.clone().path.unwrap();
    let package = Path::new(&path);
    if !package.exists() {
        println!("File {} does not exist", path);
        exit(1);
    }
    if !package.is_file() {
        println!("{} is not a file", path);
        exit(2);
    }

    if !path.ends_with(".nupkg") {
        println!("Invalid file: it is not nuget package");
        exit(3);
    }

    send_packet(&path, args_clone).await;

    exit(0);
}

fn regular_key(key: &String) {
    println!("Authenticating... {}", key);
    let mut current = read_config();

    current.key = Some(key.clone());

    write_config(current);
}

fn show_config() {
    let config = config_path();

    let mut ctg = String::new();

    match File::open(config) {
        Ok(f) => {
            let mut reader = BufReader::new(f);
            reader.read_to_string(&mut ctg).unwrap();
        }
        _ => {
            ctg = "Not configured yet".to_string();
        }
    }
    println!("{}", ctg);
}

fn logout() {
    let mut config = read_config();
    config.key = None;
    write_config(config);
}

fn find_packets(path: &Path) -> Vec<Packet> {
    let mut packets: Vec<Packet> = Vec::new();

    for r in path.read_dir().unwrap() {
        if let Ok(entry) = r {
            if !entry.path().is_file() {
                continue;
            }
            let name = entry.file_name().to_str().unwrap().to_string();
            if !name.ends_with(".nupkg") {
                continue;
            }

            packets.push(Packet::new(entry.path().as_path()))
        }
    }

    packets
}

fn check() {
    println!("Checking for newer versions");
    let config = read_config();
    println!("Found {} packets", config.packets.len());
    let mut updates = 0_u32;
    for packet in config.packets {
        let path = packet.path;
        print!("{} ({}) Current: [{}]", packet.key, path, packet.version);
        let versions = find_packets(Path::new(&path));
        if let Some(newer) = versions.iter().find(|v| v.version > packet.version) {
            println!(" -> [{}]", newer.version);
            updates += 1;
        } else {
            println!(" Lastest")
        }
    }

    if updates > 0 {
        println!("{} Updates found", updates);
    }
    if updates == 0 {
        println!("No newer versions found.");
    }
}
