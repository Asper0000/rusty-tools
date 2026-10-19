//! Standalone CLI entry point.
//!
//! Same library as the injected DLL, but runs as a normal process. Used
//! for developer tooling: parse Lua files, parse TOML configs, exercise
//! hashes, and drive the same `bst://` URI handler `rundll32` would.
//!
//! None of the subcommands require injection. They exercise the layers
//! that do not need a live Steam pipe. `init` and `parse-toml` are the
//! two that touch the config loader; both are the same code paths the
//! DLL uses, so a config that loads cleanly here loads cleanly there.

#![allow(non_snake_case)]

use std::path::Path;
use std::process::ExitCode;

use RustyTools::bridge;
use RustyTools::platform::hash;
use RustyTools::utils::config::{config, lua_config};
use RustyTools::utils::support::fnv1a_hash;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("");

    match cmd {
        ""                       => cmd_bare_init(),
        "version"                => cmd_version(),
        "init"                   => cmd_init(&args[1..]),
        "parse-lua"              => cmd_parse_lua(&args[1..]),
        "parse-toml"             => cmd_parse_toml(&args[1..]),
        "sha256"                 => cmd_sha256(&args[1..]),
        "fnv1a"                  => cmd_fnv1a(&args[1..]),
        "persist-keys"           => cmd_persist_keys(&args[1..]),
        "help" | "-h" | "--help" => { print_help(); ExitCode::SUCCESS }
        other => {
            eprintln!("ost-cli: unknown subcommand '{other}'");
            eprintln!();
            print_help();
            ExitCode::from(2)
        }
    }
}

fn cmd_bare_init() -> ExitCode {
    match RustyTools::init() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ost-cli: init failed: {e}");
            ExitCode::from(1)
        }
    }
}

fn cmd_version() -> ExitCode {
    println!("{}", RustyTools::VERSION);
    ExitCode::SUCCESS
}

fn cmd_init(rest: &[String]) -> ExitCode {
    let mut config_path: Option<&str> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--config" | "-c" => {
                i += 1;
                let Some(p) = rest.get(i) else {
                    eprintln!("ost-cli init: --config requires a path");
                    return ExitCode::from(2);
                };
                config_path = Some(p.as_str());
            }
            other => {
                eprintln!("ost-cli init: unexpected argument '{other}'");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let path = config_path.unwrap_or("rustytools.toml");
    println!("Loading config: {path}");
    let result = config::load(path);
    print_config_summary(&result);
    ExitCode::SUCCESS
}

fn cmd_parse_lua(rest: &[String]) -> ExitCode {
    let Some(path) = rest.first() else {
        eprintln!("ost-cli parse-lua: missing <PATH>");
        return ExitCode::from(2);
    };
    let p = Path::new(path);
    if p.is_dir() {
        println!("Parsing Lua directory: {path}");
        lua_config::parse_directory(path);
    } else if p.is_file() {
        println!("Parsing Lua file: {path}");
        lua_config::parse_file(path);
    } else {
        eprintln!("ost-cli parse-lua: {path} is not a file or directory");
        return ExitCode::from(2);
    }
    print_lua_summary();
    ExitCode::SUCCESS
}

fn cmd_parse_toml(rest: &[String]) -> ExitCode {
    let Some(path) = rest.first() else {
        eprintln!("ost-cli parse-toml: missing <PATH>");
        return ExitCode::from(2);
    };
    println!("Parsing TOML config: {path}");
    let result = config::load(path);
    print_config_summary(&result);
    ExitCode::SUCCESS
}

fn cmd_sha256(rest: &[String]) -> ExitCode {
    let Some(path) = rest.first() else {
        eprintln!("ost-cli sha256: missing <PATH>");
        return ExitCode::from(2);
    };
    let h = hash::sha256_of_file(Path::new(path));
    if h.is_empty() {
        eprintln!("ost-cli sha256: cannot read {path}");
        return ExitCode::from(2);
    }
    println!("{h}");
    ExitCode::SUCCESS
}

fn cmd_persist_keys(rest: &[String]) -> ExitCode {
    let mut steam_path: Option<&str> = None;
    let mut lua_path: Option<&str> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--steam" | "-s" => {
                i += 1;
                steam_path = rest.get(i).map(|s| s.as_str());
            }
            "--lua" | "-l" => {
                i += 1;
                lua_path = rest.get(i).map(|s| s.as_str());
            }
            other => {
                eprintln!("ost-cli persist-keys: unexpected argument '{other}'");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let Some(steam) = steam_path else {
        eprintln!("ost-cli persist-keys: --steam <path> is required");
        return ExitCode::from(2);
    };
    let Some(lua) = lua_path else {
        eprintln!("ost-cli persist-keys: --lua <path> is required");
        return ExitCode::from(2);
    };

    println!("Parsing Lua from: {lua}");
    lua_config::parse_directory(lua);
    let depots = lua_config::get_all_depot_ids();
    println!("Found {} depot(s) in Lua config", depots.len());

    println!("Persisting to: {steam}\\config\\config.vdf");
    let ok = RustyTools::utils::config::vdf_persist::persist_lua_keys_to_config_vdf(steam);
    if ok {
        let path = std::path::Path::new(steam).join("config").join("config.vdf");
        let count = std::fs::read_to_string(&path)
            .map(|t| t.matches("\"DecryptionKey\"").count())
            .unwrap_or(0);
        println!("OK ({} DecryptionKey entries in config.vdf)", count);
        ExitCode::SUCCESS
    } else {
        eprintln!("FAILED");
        ExitCode::from(1)
    }
}
fn cmd_fnv1a(rest: &[String]) -> ExitCode {
    let Some(s) = rest.first() else {
        eprintln!("ost-cli fnv1a: missing <STRING>");
        return ExitCode::from(2);
    };
    let h = fnv1a_hash(s);
    println!("0x{h:08X}  ({h})");
    ExitCode::SUCCESS
}

fn print_config_summary(result: &config::LoadResult) {
    println!("  applied:           {}", result.applied);
    println!("  lua_paths_changed: {}", result.lua_paths_changed);
    println!("  log.level:         {}", config::get_log_level().as_str());
    println!("  lua.paths:         {} entr(ies)", config::get_lua_paths().len());
    println!("  stats.enable_api:  {}", config::get_stats_enable_api());
    println!("  update.enabled:    {}", config::get_update_enabled());
    let t = config::get_manifest_timeouts();
    println!(
        "  manifest timeouts: resolve={} connect={} send={} recv={}",
        t.resolve, t.connect, t.send, t.recv
    );
    let c = config::get_cloud_settings();
    println!("  cloud.enabled:     {} (library='{}')", c.enabled, c.library);
    println!("  manifest provider: {}", bridge::manifest_active_provider_name());
}

fn print_lua_summary() {
    let depots = lua_config::get_all_depot_ids();
    let with_keys = depots
        .iter()
        .filter(|id| !lua_config::get_decryption_key(**id).is_empty())
        .count();
    let with_tokens = depots
        .iter()
        .filter(|id| lua_config::get_access_token(**id) != 0)
        .count();
    println!("  depot(s) registered:     {}", depots.len());
    println!("  depot(s) with AES key:   {with_keys}");
    println!("  appid(s) with token:     {with_tokens}");
    println!("  fetch_manifest_code:     {}", lua_config::has_manifest_code_func());
    println!("  fetch_manifest_code_ex:  {}", lua_config::has_manifest_code_func_ex());
    println!(
        "  eticket url configured:  {}",
        !lua_config::get_eticket_url().is_empty()
    );
}

fn print_help() {
    println!(
        "\
ost-cli - rusty_tools CLI

USAGE:
    ost-cli <SUBCOMMAND>

SUBCOMMANDS:
    (none)                Initialize the runtime and exit cleanly.
    version               Print the library version.
    init [--config PATH]  Load a TOML config file (default:
                          rustytools.toml) and print the resulting
                          settings.
    parse-lua PATH        Parse a Lua file or directory and print the
                          resulting depot / token / override state.
    parse-toml PATH       Parse a TOML config file and print the settings
                          the loader would apply.
    sha256 PATH           Print the lowercase-hex SHA-256 of a file.
    persist-keys --steam PATH --lua PATH
                          Parse a Lua directory and write all depot
                          decryption keys into <steam>/config/config.vdf
                          under a synthetic apps/0 node, for external
                          tools (e.g. blueprint extractors) to consume.
    fnv1a STRING          Print the FNV-1a 32-bit hash of a string (the
                          pattern-loader key format).
    help, -h, --help      Print this help.
"
    );
}
