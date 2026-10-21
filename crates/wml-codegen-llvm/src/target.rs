use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TargetArch {
    X86_64,
    AArch64,
}

impl fmt::Display for TargetArch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TargetArch::X86_64 => write!(f, "x86_64"),
            TargetArch::AArch64 => write!(f, "aarch64"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TargetOs {
    Linux,
    Windows,
    Darwin,
}

impl fmt::Display for TargetOs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TargetOs::Linux => write!(f, "linux"),
            TargetOs::Windows => write!(f, "windows"),
            TargetOs::Darwin => write!(f, "darwin"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TargetConfig {
    pub arch: TargetArch,
    pub os: TargetOs,
}

impl Default for TargetConfig {
    fn default() -> Self {
        Self::host()
    }
}

impl TargetConfig {
    pub fn host() -> Self {
        let arch = if cfg!(target_arch = "aarch64") {
            TargetArch::AArch64
        } else {
            TargetArch::X86_64
        };
        let os = if cfg!(target_os = "windows") {
            TargetOs::Windows
        } else if cfg!(target_os = "macos") {
            TargetOs::Darwin
        } else {
            TargetOs::Linux
        };
        Self { arch, os }
    }

    pub fn x86_64_linux() -> Self {
        Self {
            arch: TargetArch::X86_64,
            os: TargetOs::Linux,
        }
    }

    pub fn aarch64_linux() -> Self {
        Self {
            arch: TargetArch::AArch64,
            os: TargetOs::Linux,
        }
    }

    pub fn x86_64_windows() -> Self {
        Self {
            arch: TargetArch::X86_64,
            os: TargetOs::Windows,
        }
    }

    pub fn parse(target_str: &str) -> Option<Self> {
        let s = target_str.to_lowercase();
        let arch = if s.contains("aarch64") || s.contains("arm64") {
            TargetArch::AArch64
        } else if s.contains("x86_64") || s.contains("amd64") || s.contains("x64") {
            TargetArch::X86_64
        } else {
            return None;
        };

        let os = if s.contains("windows") || s.contains("win32") {
            TargetOs::Windows
        } else if s.contains("darwin") || s.contains("macos") || s.contains("apple") {
            TargetOs::Darwin
        } else {
            TargetOs::Linux
        };

        Some(Self { arch, os })
    }

    pub fn triple(&self) -> String {
        match (self.arch, self.os) {
            (TargetArch::X86_64, TargetOs::Linux) => "x86_64-unknown-linux-gnu".to_string(),
            (TargetArch::AArch64, TargetOs::Linux) => "aarch64-unknown-linux-gnu".to_string(),
            (TargetArch::X86_64, TargetOs::Windows) => "x86_64-pc-windows-msvc".to_string(),
            (TargetArch::AArch64, TargetOs::Windows) => "aarch64-pc-windows-msvc".to_string(),
            (TargetArch::X86_64, TargetOs::Darwin) => "x86_64-apple-darwin".to_string(),
            (TargetArch::AArch64, TargetOs::Darwin) => "aarch64-apple-darwin".to_string(),
        }
    }

    pub fn datalayout(&self) -> String {
        match (self.arch, self.os) {
            (TargetArch::X86_64, TargetOs::Windows) => {
                "e-m:w-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
                    .to_string()
            }
            (TargetArch::X86_64, _) => {
                "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
                    .to_string()
            }
            (TargetArch::AArch64, TargetOs::Darwin) => {
                "e-m:o-i64:64-i128:128-n32:64-S128".to_string()
            }
            (TargetArch::AArch64, _) => {
                "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128".to_string()
            }
        }
    }
}
