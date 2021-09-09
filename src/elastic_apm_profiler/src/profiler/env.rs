// Licensed to Elasticsearch B.V under
// one or more agreements.
// Elasticsearch B.V licenses this file to you under the Apache 2.0 License.
// See the LICENSE file in the project root for more information

use crate::{
    ffi::{COR_PRF_CLAUSE_TYPE::COR_PRF_CLAUSE_FILTER, E_FAIL},
    profiler::types::Integration,
};
use com::sys::HRESULT;
use log::LevelFilter;
use log4rs::{
    append::{
        console::ConsoleAppender,
        rolling_file::{
            policy::compound::{
                roll::fixed_window::FixedWindowRoller, trigger::size::SizeTrigger, CompoundPolicy,
            },
            RollingFileAppender,
        },
    },
    config::{Appender, Logger, Root},
    encode::pattern::PatternEncoder,
    Config, Handle,
};
use once_cell::sync::Lazy;
use std::{
    borrow::Borrow,
    collections::{BTreeMap, HashMap, HashSet},
    ffi::OsStr,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    str::FromStr,
};

const ELASTIC_APM_PROFILER_INTEGRATIONS: &str = "ELASTIC_APM_PROFILER_INTEGRATIONS";
const ELASTIC_APM_PROFILER_LOG_TARGETS_ENV_VAR: &str = "ELASTIC_APM_PROFILER_LOG_TARGETS";
const ELASTIC_APM_PROFILER_LOG_ENV_VAR: &str = "ELASTIC_APM_PROFILER_LOG";
const ELASTIC_APM_PROFILER_LOG_DIR_ENV_VAR: &str = "ELASTIC_APM_PROFILER_LOG_DIR";
const ELASTIC_APM_PROFILER_LOG_IL_ENV_VAR: &str = "ELASTIC_APM_PROFILER_LOG_IL";
const ELASTIC_APM_PROFILER_CALLTARGET_ENABLED_ENV_VAR: &str =
    "ELASTIC_APM_PROFILER_CALLTARGET_ENABLED";
const ELASTIC_APM_PROFILER_ENABLE_INLINING: &str = "ELASTIC_APM_PROFILER_ENABLE_INLINING";
const ELASTIC_APM_PROFILER_DISABLE_OPTIMIZATIONS: &str =
    "ELASTIC_APM_PROFILER_DISABLE_OPTIMIZATIONS";

pub static ELASTIC_APM_PROFILER_LOG_IL: Lazy<bool> =
    Lazy::new(|| read_bool_env_var(ELASTIC_APM_PROFILER_LOG_IL_ENV_VAR, false));

pub static ELASTIC_APM_PROFILER_CALLTARGET_ENABLED: Lazy<bool> =
    Lazy::new(|| read_bool_env_var(ELASTIC_APM_PROFILER_CALLTARGET_ENABLED_ENV_VAR, true));

/// Gets the environment variables of interest
pub fn get_env_vars() -> String {
    std::env::vars()
        .filter_map(|(k, v)| {
            if k.starts_with("ELASTIC_") || k.starts_with("CORECLR_") || k.starts_with("COR_") {
                Some(format!("  {}=\"{}\"", k, v))
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Gets the path to the profiler file on windows
#[cfg(target_os = "windows")]
pub fn get_native_profiler_file() -> Result<String, HRESULT> {
    Ok("elastic_apm_profiler.dll".into())
}

/// Gets the path to the profiler file on non windows
#[cfg(not(target_os = "windows"))]
pub fn get_native_profiler_file() -> Result<String, HRESULT> {
    let env_var = if cfg!(target_pointer_width = "64") {
        "CORECLR_PROFILER_PATH_64"
    } else {
        "CORECLR_PROFILER_PATH_32"
    };
    match std::env::var(env_var) {
        Ok(v) => Ok(v),
        Err(_) => std::env::var("CORECLR_PROFILER_PATH").map_err(|e| {
            log::warn!(
                "problem getting env var CORECLR_PROFILER_PATH: {}",
                e.to_string()
            );
            E_FAIL
        }),
    }
}

pub fn disable_optimizations() -> bool {
    read_bool_env_var(ELASTIC_APM_PROFILER_DISABLE_OPTIMIZATIONS, false)
}

pub fn enable_inlining(default: bool) -> bool {
    read_bool_env_var(ELASTIC_APM_PROFILER_ENABLE_INLINING, default)
}

fn read_log_targets_from_env_var() -> HashSet<String> {
    let mut set = match std::env::var(ELASTIC_APM_PROFILER_LOG_TARGETS_ENV_VAR) {
        Ok(value) => value
            .split(';')
            .into_iter()
            .filter_map(|s| match s.to_lowercase().as_str() {
                out if out == "file" || out == "stdout" => Some(out.into()),
                _ => None,
            })
            .collect(),
        _ => HashSet::with_capacity(1),
    };

    if set.is_empty() {
        set.insert("file".into());
    }
    set
}

pub fn read_log_level_from_env_var(default: LevelFilter) -> LevelFilter {
    match std::env::var(ELASTIC_APM_PROFILER_LOG_ENV_VAR) {
        Ok(value) => LevelFilter::from_str(value.as_str()).unwrap_or(default),
        _ => default,
    }
}

fn read_bool_env_var(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(enabled) => match enabled.to_lowercase().as_str() {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => {
                log::info!(
                    "Unknown value for {}: {}. Setting to {}",
                    key,
                    enabled,
                    default
                );
                default
            }
        },
        Err(e) => {
            log::info!(
                "Problem reading {}: {}. Setting to {}",
                key,
                e.to_string(),
                default
            );
            default
        }
    }
}

/// get the profiler directory
fn get_profiler_dir() -> String {
    let env_var = if cfg!(target_pointer_width = "64") {
        "CORECLR_PROFILER_PATH_64"
    } else {
        "CORECLR_PROFILER_PATH_32"
    };

    match std::env::var(env_var) {
        Ok(v) => v,
        Err(_) => match std::env::var("CORECLR_PROFILER_PATH") {
            Ok(v) => v,
            Err(_) => {
                // try .NET Framework env vars
                let env_var = if cfg!(target_pointer_width = "64") {
                    "COR_PROFILER_PATH_64"
                } else {
                    "COR_PROFILER_PATH_32"
                };

                match std::env::var(env_var) {
                    Ok(v) => v,
                    Err(_) => std::env::var("COR_PROFILER_PATH").unwrap_or_else(|_| String::new()),
                }
            }
        },
    }
}

/// Gets the default log directory on Windows
#[cfg(target_os = "windows")]
pub fn get_default_log_dir() -> PathBuf {
    // ideally we would use the windows function SHGetKnownFolderPath to get
    // the CommonApplicationData special folder. However, this requires a few package dependencies
    // like winapi that would increase the size of the profiler binary. Instead,
    // use the %PROGRAMDATA% environment variable if it exists
    match std::env::var("PROGRAMDATA") {
        Ok(path) => {
            let mut path_buf = PathBuf::from(path);
            path_buf = path_buf
                .join("elastic")
                .join("apm-agent-dotnet")
                .join("logs");
            path_buf
        }
        Err(_) => {
            let mut path_buf = PathBuf::from(get_profiler_dir());
            path_buf = path_buf.join("logs");
            path_buf
        }
    }
}

/// Gets the path to the profiler file on non windows
#[cfg(not(target_os = "windows"))]
pub fn get_default_log_dir() -> PathBuf {
    PathBuf::from_str("/var/log/elastic/apm-agent-dotnet").unwrap()
}

fn get_log_dir() -> PathBuf {
    match std::env::var(ELASTIC_APM_PROFILER_LOG_DIR_ENV_VAR) {
        Ok(path) => PathBuf::from(path),
        Err(_) => get_default_log_dir(),
    }
}

pub fn initialize_logging(process_name: &str) -> Handle {
    let targets = read_log_targets_from_env_var();
    let level = read_log_level_from_env_var(LevelFilter::Warn);
    let mut root_builder = Root::builder();
    let mut config_builder = Config::builder();
    let log_pattern = "[{d(%Y-%m-%dT%H:%M:%S.%f%:z)}] [{l:<5}] {m}{n}";

    if targets.contains("stdout") {
        let pattern = PatternEncoder::new(log_pattern);
        let stdout = ConsoleAppender::builder()
            .encoder(Box::new(pattern))
            .build();
        config_builder =
            config_builder.appender(Appender::builder().build("stdout", Box::new(stdout)));
        root_builder = root_builder.appender("stdout");
    }

    if targets.contains("file") {
        let pid = std::process::id();
        let mut log_dir = get_log_dir();
        let mut valid_log_dir = true;
        if log_dir.exists() && !log_dir.is_dir() {
            log_dir = get_default_log_dir();
        }

        if !log_dir.exists() {
            // try to create the log directory ahead of time so that we can determine if it's a valid
            // directory. if the directory can't be created, try the default log directory before
            // bailing and not setting up the file logger.
            if let Err(_) = std::fs::create_dir_all(&log_dir) {
                if log_dir != get_default_log_dir() {
                    log_dir = get_default_log_dir();
                    if let Err(_) = std::fs::create_dir_all(&log_dir) {
                        valid_log_dir = false;
                    }
                }
            }
        }

        if valid_log_dir {
            let log_file_name = log_dir
                .clone()
                .join(format!("elastic_apm_profiler_{}_{}.log", process_name, pid))
                .to_string_lossy()
                .to_string();
            let rolling_log_file_name = log_dir
                .clone()
                .join(format!(
                    "elastic_apm_profiler_{}_{}_{{}}.log",
                    process_name, pid
                ))
                .to_string_lossy()
                .to_string();

            let trigger = SizeTrigger::new(5 * 1024 * 1024);
            let roller = FixedWindowRoller::builder()
                .build(&rolling_log_file_name, 10)
                .unwrap();

            let policy = CompoundPolicy::new(Box::new(trigger), Box::new(roller));
            let pattern = PatternEncoder::new(log_pattern);
            let file = RollingFileAppender::builder()
                .append(true)
                .encoder(Box::new(pattern))
                .build(&log_file_name, Box::new(policy))
                .unwrap();

            config_builder =
                config_builder.appender(Appender::builder().build("file", Box::new(file)));
            root_builder = root_builder.appender("file");
        }
    }

    let root = root_builder.build(level);
    let config = config_builder.build(root).unwrap();
    log4rs::init_config(config).unwrap()
}

/// Loads the integrations by reading the yml file pointed to
/// by [ELASTIC_APM_PROFILER_INTEGRATIONS] environment variable
pub fn load_integrations() -> Result<Vec<Integration>, HRESULT> {
    let path = std::env::var(ELASTIC_APM_PROFILER_INTEGRATIONS).map_err(|e| {
        log::warn!(
            "Problem reading {} environment variable: {}. profiler is disabled.",
            ELASTIC_APM_PROFILER_INTEGRATIONS,
            e.to_string()
        );
        E_FAIL
    })?;

    let file = File::open(&path).map_err(|e| {
        log::warn!(
            "Problem reading integrations file {}: {}. profiler is disabled.",
            &path,
            e.to_string()
        );
        E_FAIL
    })?;

    let reader = BufReader::new(file);
    let integrations = serde_yaml::from_reader(reader).map_err(|e| {
        log::warn!(
            "Problem reading integrations file {}: {}. profiler is disabled.",
            &path,
            e.to_string()
        );
        E_FAIL
    })?;

    Ok(integrations)
}