use anyhow::{Result, anyhow};
use qrcode::QrCode;
use qrcode::render::unicode;
use rand::distributions::{Alphanumeric, DistString};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct PairingQr {
    pub instance: String,
    pub secret: String,
    pub payload: String,
}

impl PairingQr {
    pub fn generate() -> Self {
        Self::with_instance(pairing_instance_name())
    }

    fn with_instance(instance: String) -> Self {
        let secret = safe_random(16);
        let payload = format!("WIFI:T:ADB;S:{instance};P:{secret};;");

        Self {
            instance,
            secret,
            payload,
        }
    }

    pub fn render_terminal(&self) -> Result<String> {
        let code = QrCode::new(self.payload.as_bytes())
            .map_err(|error| anyhow!("failed to build QR code: {error}"))?;

        Ok(code.render::<unicode::Dense1x2>().quiet_zone(true).build())
    }

    pub fn modules(&self) -> Result<QrModules> {
        let code = QrCode::new(self.payload.as_bytes())
            .map_err(|error| anyhow!("failed to build QR code: {error}"))?;
        let size = code.width();

        Ok(QrModules {
            size,
            dark: code
                .to_colors()
                .iter()
                .map(|color| color.select(true, false))
                .collect(),
        })
    }
}

/// Square QR matrix in row-major order; `dark[y * size + x]` is true for dark modules.
#[derive(Debug, Clone)]
pub struct QrModules {
    pub size: usize,
    pub dark: Vec<bool>,
}

fn pairing_instance_name() -> String {
    // A unique per-session suffix: pairing discovers the phone by matching this
    // mDNS instance, so a deterministic (hostname-only) name could collide with
    // a stale advertisement and pair against the wrong endpoint.
    let suffix = safe_random(6).to_ascii_lowercase();
    let base = machine_name_candidates()
        .into_iter()
        .find_map(|candidate| sanitize_pairing_instance(&candidate))
        .unwrap_or_else(|| "awb".to_string());
    let base: String = base.chars().take(56).collect();
    format!("{base}-{suffix}")
}

fn machine_name_candidates() -> Vec<String> {
    let mut candidates = Vec::new();

    if let Some(name) = command_stdout("scutil", ["--get", "LocalHostName"]) {
        candidates.push(name);
    }

    if let Some(name) = command_stdout("scutil", ["--get", "ComputerName"]) {
        candidates.push(name);
    }

    if let Some(name) = command_stdout("hostname", ["-s"]) {
        candidates.push(name);
    }

    if let Ok(name) = std::env::var("HOSTNAME") {
        candidates.push(name);
    }

    candidates
}

fn command_stdout<const N: usize>(program: &str, args: [&str; N]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;

    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if value.is_empty() { None } else { Some(value) }
}

fn sanitize_pairing_instance(value: &str) -> Option<String> {
    let mut sanitized = String::new();
    let mut last_was_separator = false;

    for character in value.trim().trim_end_matches(".local").chars() {
        if character.is_ascii_alphanumeric() {
            sanitized.push(character.to_ascii_lowercase());
            last_was_separator = false;
        } else if matches!(character, '-' | '_' | '.' | ' ')
            && !sanitized.is_empty()
            && !last_was_separator
        {
            sanitized.push('-');
            last_was_separator = true;
        }
    }

    while sanitized.ends_with('-') {
        sanitized.pop();
    }

    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized.chars().take(63).collect())
    }
}

fn safe_random(length: usize) -> String {
    Alphanumeric.sample_string(&mut rand::thread_rng(), length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_adb_wifi_qr_payload() {
        let qr = PairingQr::with_instance("ovi-m1-001".to_string());

        assert_eq!(qr.instance, "ovi-m1-001");
        assert_eq!(qr.secret.len(), 16);
        assert_eq!(
            qr.payload,
            format!("WIFI:T:ADB;S:{};P:{};;", qr.instance, qr.secret)
        );
        assert!(
            qr.payload
                .chars()
                .all(|character| character.is_ascii_alphanumeric()
                    || matches!(character, ':' | ';' | '-' | '_'))
        );
    }

    #[test]
    fn generated_instances_are_unique_per_session() {
        assert_ne!(
            PairingQr::generate().instance,
            PairingQr::generate().instance
        );
    }

    #[test]
    fn sanitizes_machine_names_for_qr_instances() {
        assert_eq!(
            sanitize_pairing_instance("Ovi M1 001.local").as_deref(),
            Some("ovi-m1-001")
        );
        assert_eq!(
            sanitize_pairing_instance(" Ovi's MacBook Pro ").as_deref(),
            Some("ovis-macbook-pro")
        );
        assert_eq!(sanitize_pairing_instance("...").as_deref(), None);
    }
}
