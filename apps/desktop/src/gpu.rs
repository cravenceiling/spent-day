use std::process::{Command, ExitCode, Stdio};

pub fn check() -> Result<(), ExitCode> {
    // Rune swallows window GPU errors and exits inside headless rendering, before returning.
    rune::render::Gpu::try_new(rune::render::Gpu::instance(), None)
        .map(|_| ())
        .map_err(|error| {
            let release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
            let message = format!(
                "Spent Day cannot render because no usable Vulkan GPU driver is available.\n{}\nDetails: {error}",
                driver_help(&release)
            );
            eprintln!("spentday: {message}");
            let _ = Command::new("notify-send")
                .args(["--app-name=Spent Day", "--urgency=critical", "Spent Day cannot start", &message])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            ExitCode::from(4)
        })
}

fn driver_help(release: &str) -> &'static str {
    let families: Vec<_> = release
        .lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| matches!(*key, "ID" | "ID_LIKE"))
        .flat_map(|(_, value)| value.trim_matches(['\'', '"']).split_whitespace())
        .collect();
    if families.contains(&"arch") {
        "Install the driver for your GPU: sudo pacman -S vulkan-intel (Intel), sudo pacman -S vulkan-radeon (AMD), or sudo pacman -S nvidia-utils (NVIDIA). Without a GPU, use sudo pacman -S vulkan-swrast for software rendering."
    } else if families.contains(&"debian") || families.contains(&"ubuntu") {
        "Install a Vulkan driver: sudo apt install mesa-vulkan-drivers"
    } else if families.contains(&"fedora") {
        "Install a Vulkan driver: sudo dnf install mesa-vulkan-drivers"
    } else {
        "Install your GPU's Vulkan driver, then open Spent Day again."
    }
}

#[cfg(test)]
mod tests {
    use super::driver_help;

    #[test]
    fn driver_instructions_use_id_and_id_like() {
        for (release, expected) in [
            ("ID=arch\n", "vulkan-swrast"),
            ("ID=endeavouros\nID_LIKE=\"arch\"\n", "vulkan-intel"),
            ("ID=ubuntu\nID_LIKE=debian\n", "sudo apt install"),
            ("ID=custom\nID_LIKE='ubuntu debian'\n", "sudo apt install"),
            ("ID=fedora\n", "sudo dnf install"),
            (
                "ID=custom\nID_LIKE=archipelago\n",
                "Install your GPU's Vulkan driver",
            ),
        ] {
            assert!(driver_help(release).contains(expected), "{release}");
        }
    }
}
