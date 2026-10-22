use std::{error::Error, path::PathBuf};

use lucent_lab::{Lab, Viewport, render};

const HELP: &str = "lucent-lab [--story ID] [--palette ID] [--state ID] [--viewport regular|narrow]\n\
lucent-lab --render NEW_OUTPUT_DIRECTORY [--source-commit FULL_GIT_SHA]\n\
Native: production controls in a centered layer-shell surface; no desktop is replaced.\n\
Render: the complete committed catalogue, using one offscreen GPU sequentially.\n\
Stories: buttons,input,input-group,toggle,slider,scroll,popover,menu,motion";

#[derive(Default)]
struct Arguments {
    story: Option<String>,
    palette: Option<String>,
    state: Option<String>,
    viewport: Option<String>,
    output: Option<PathBuf>,
    source: Option<String>,
    help: bool,
}
impl Arguments {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut args = args.into_iter();
        let mut result = Self::default();
        while let Some(arg) = args.next() {
            if arg == "--help" {
                result.help = true;
                continue;
            }
            let value = args
                .next()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("Missing value for {arg}"))?;
            let slot = match arg.as_str() {
                "--story" => &mut result.story,
                "--palette" => &mut result.palette,
                "--state" => &mut result.state,
                "--viewport" => &mut result.viewport,
                "--source-commit" => &mut result.source,
                "--render" => {
                    if result.output.replace(PathBuf::from(value)).is_some() {
                        return Err("Repeated --render".into());
                    }
                    continue;
                }
                _ => return Err(format!("Unknown argument {arg}")),
            };
            if slot.replace(value).is_some() {
                return Err(format!("Repeated {arg}"));
            }
        }
        if result.source.is_some() && result.output.is_none() {
            return Err("--source-commit requires --render".into());
        }
        if result.output.is_some()
            && (result.story.is_some()
                || result.palette.is_some()
                || result.state.is_some()
                || result.viewport.is_some())
        {
            return Err(
                "--render exports the complete catalogue; native selection flags cannot filter it"
                    .into(),
            );
        }
        Ok(result)
    }
    fn lab(&self) -> Result<Lab, String> {
        Lab::new(
            self.story.as_deref().unwrap_or("buttons"),
            self.palette.as_deref().unwrap_or("dark"),
            self.state.as_deref(),
            Viewport::parse(self.viewport.as_deref().unwrap_or("regular"))?,
        )
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Arguments::parse(std::env::args().skip(1))?;
    if args.help {
        println!("{HELP}");
        return Ok(());
    }
    if let Some(output) = &args.output {
        render::export(output, args.source.as_deref())
    } else {
        lucent_wayland::run(args.lab()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(args: &[&str]) -> Result<Arguments, String> {
        Arguments::parse(args.iter().map(|value| (*value).into()))
    }
    #[test]
    fn native_cli_validates_story_palette_state_and_viewport_before_runtime() {
        assert!(parse(&[]).unwrap().lab().is_ok());
        for args in [
            &["--story", "unknown"][..],
            &["--palette", "unknown"],
            &["--state", "unknown"],
            &["--viewport", "unknown"],
        ] {
            assert!(parse(args).unwrap().lab().is_err());
        }
        assert!(
            parse(&[
                "--story",
                "motion",
                "--state",
                "middle",
                "--viewport",
                "narrow"
            ])
            .unwrap()
            .lab()
            .is_ok()
        );
    }
    #[test]
    fn export_does_not_silently_ignore_native_flags_or_repeat_options() {
        for args in [
            &["--render", "out", "--story", "buttons"][..],
            &["--source-commit", "abc"],
            &["--story", "buttons", "--story", "menu"],
            &["--render"],
            &["--unknown", "value"],
        ] {
            assert!(parse(args).is_err());
        }
    }
}
