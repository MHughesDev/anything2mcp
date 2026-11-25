//! The completion command: a script that completes this program's commands
//! and flags in a person's shell, made from the command tree the help is
//! printed from.
//!
//! The script holds every word it offers. Nothing in it runs the program
//! when a word is completed, so completing starts no engine and reads
//! nothing; and the values that live in the engine, such as the names of
//! targets and tools, are not offered, which its header says.

use a2m_actions::{CommandFace, ExitClass, PROGRAM, codes};
use a2m_core::{Failure, FailureClass};

use super::parse::{
    COMPLETION_COMMAND, GLOBAL_FLAGS, GLOBAL_VALUES, Tree, process_commands, process_flags,
};
use super::{Finished, stopped};

/// The shells a script is made for, by the word the command takes.
pub(crate) const SHELLS: [&str; 4] = ["bash", "zsh", "fish", "powershell"];

/// The short name of the program, which the scripts complete as well.
const SHORT_NAME: &str = "a2m";

/// The most characters of a description in a script.
const DESCRIPTION_LIMIT: usize = 100;

/// A word the script offers: only these characters, so that no word can
/// be read by a shell as anything but itself.
fn plain(word: &str) -> bool {
    !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// A description on one line, cut to its limit.
fn described(text: &str) -> String {
    let one: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    if one.chars().count() <= DESCRIPTION_LIMIT {
        return one;
    }
    let cut: String = one.chars().take(DESCRIPTION_LIMIT - 3).collect();
    format!("{}...", cut.trim_end())
}

/// A flag's name alone, from its form in the global list, such as
/// `--project <folder>` or `--help, -h`.
fn flag_names(form: &str) -> Vec<&str> {
    form.split(", ")
        .filter_map(|part| part.split_whitespace().next())
        .collect()
}

/// What a script offers, from the tree.
struct Words {
    /// The first word: nouns, shortcuts, process commands, with what each is.
    top: Vec<(String, String)>,
    /// The verbs of each noun, and the shells of the completion command.
    second: Vec<(String, Vec<(String, String)>)>,
    /// The flags of each command, by its words: `noun verb`, or one word.
    flags: Vec<(String, Vec<(String, String)>)>,
    /// The global flags.
    globals: Vec<(String, String)>,
}

impl Words {
    fn of(tree: &Tree) -> Self {
        let mut top = Vec::new();
        let mut second = Vec::new();
        let mut flags = Vec::new();
        for noun in tree.nouns() {
            let faces: Vec<&CommandFace> = tree
                .faces()
                .iter()
                .filter(|face| face.noun == noun && plain(&face.verb))
                .collect();
            if !plain(noun) || faces.is_empty() {
                continue;
            }
            let verbs: Vec<&str> = faces.iter().map(|face| face.verb.as_str()).collect();
            top.push((noun.to_owned(), described(&verbs.join(", "))));
            let mut of_noun = Vec::new();
            for face in faces {
                let help = if tree.built(&face.action) {
                    described(&face.help)
                } else {
                    described(&format!("(not built) {}", face.help))
                };
                of_noun.push((face.verb.clone(), help));
                let of_face: Vec<(String, String)> = face
                    .args
                    .iter()
                    .filter(|arg| !arg.positional && plain(&arg.flag))
                    .map(|arg| (arg.flag.clone(), described(&arg.help)))
                    .collect();
                if !of_face.is_empty() {
                    flags.push((format!("{noun} {}", face.verb), of_face));
                }
            }
            second.push((noun.to_owned(), of_noun));
        }
        for shortcut in tree.shortcuts() {
            if plain(shortcut.word) {
                top.push((shortcut.word.to_owned(), described(shortcut.does)));
            }
        }
        for word in process_commands() {
            let about = match word {
                "serve" => "Run the local engine in the foreground.",
                "stdio" => "The program an MCP client starts.",
                COMPLETION_COMMAND => "Print a completion script for a shell.",
                _ => "Not built in this build.",
            };
            top.push((word.to_owned(), about.to_owned()));
            let own: Vec<(String, String)> = process_flags(word)
                .iter()
                .map(|(flag, does)| ((*flag).to_owned(), described(does)))
                .collect();
            if !own.is_empty() {
                flags.push((word.to_owned(), own));
            }
        }
        second.push((
            COMPLETION_COMMAND.to_owned(),
            SHELLS
                .iter()
                .map(|shell| ((*shell).to_owned(), format!("A script for {shell}.")))
                .collect(),
        ));
        let globals = GLOBAL_FLAGS
            .iter()
            .flat_map(|(form, does)| {
                flag_names(form)
                    .into_iter()
                    .map(|name| (name.to_owned(), described(does)))
            })
            .collect();
        Self {
            top,
            second,
            flags,
            globals,
        }
    }
}

/// The comment every script begins with.
fn header(shell: &str, comment: &str, load: &str) -> String {
    let lines = [
        format!(
            "Completion of {PROGRAM} and {SHORT_NAME} for {shell}, made by {PROGRAM} {} from the command tree of this build.",
            crate::version()
        ),
        "It completes commands and flags. Values that live in the engine, such as the names of targets and tools, are not completed.".to_owned(),
        format!("Nothing in it runs {PROGRAM}: every word it offers is written here. Made again after an upgrade, it offers the new build's words."),
        format!("Load it with: {load}"),
    ];
    lines
        .iter()
        .map(|line| format!("{comment} {line}\n"))
        .collect()
}

/// Text in single quotes, for bash and zsh: a quote inside ends the
/// quoting, is written escaped, and starts it again.
fn quoted_posix(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Text in single quotes, for fish, which escapes a backslash and a quote.
fn quoted_fish(text: &str) -> String {
    format!("'{}'", text.replace('\\', r"\\").replace('\'', r"\'"))
}

/// Text in single quotes, for PowerShell, which doubles a quote and takes
/// the typographic single quotes for quotes too.
fn quoted_powershell(text: &str) -> String {
    let text: String = text
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}' => '\'',
            other => other,
        })
        .collect();
    format!("'{}'", text.replace('\'', "''"))
}

fn names(words: &[(String, String)]) -> String {
    words
        .iter()
        .map(|(word, _)| word.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The loop every POSIX-like script begins its function with: the words
/// before the cursor that are not flags, the first two of them kept.
fn posix_words(first: usize, cursor: &str, word_at: &str) -> String {
    let values = GLOBAL_VALUES.join("|");
    format!(
        "    local noun='' verb='' skip='' i word
    for (( i = {first}; i < {cursor}; i++ )); do
        word={word_at}
        if [[ -n $skip ]]; then
            skip=''
            continue
        fi
        case $word in
            {values}) skip=1; continue ;;
            -*) continue ;;
        esac
        if [[ -z $noun ]]; then
            noun=$word
        elif [[ -z $verb ]]; then
            verb=$word
        fi
    done
"
    )
}

fn bash(words: &Words) -> String {
    let mut out = format!(
        "{}_a2m_complete() {{\n    local cur=${{COMP_WORDS[COMP_CWORD]}}\n",
        header("bash", "#", "source <(anything2mcp completion bash)")
    );
    out.push_str(&posix_words(1, "COMP_CWORD", "${COMP_WORDS[i]}"));
    out.push_str("    local offered=''\n    if [[ $cur == -* ]]; then\n");
    out.push_str(&format!(
        "        offered={}\n",
        quoted_posix(&names(&words.globals))
    ));
    out.push_str("        case \"$noun $verb\" in\n");
    for (command, flags) in words.flags.iter().filter(|(c, _)| c.contains(' ')) {
        out.push_str(&format!(
            "            {}) offered=\"$offered \"{} ;;\n",
            quoted_posix(command),
            quoted_posix(&names(flags))
        ));
    }
    out.push_str("        esac\n        case $noun in\n");
    for (command, flags) in words.flags.iter().filter(|(c, _)| !c.contains(' ')) {
        out.push_str(&format!(
            "            {}) offered=\"$offered \"{} ;;\n",
            quoted_posix(command),
            quoted_posix(&names(flags))
        ));
    }
    out.push_str("        esac\n    elif [[ -z $noun ]]; then\n");
    out.push_str(&format!(
        "        offered={}\n",
        quoted_posix(&names(&words.top))
    ));
    out.push_str("    elif [[ -z $verb ]]; then\n        case $noun in\n");
    for (noun, verbs) in &words.second {
        out.push_str(&format!(
            "            {}) offered={} ;;\n",
            quoted_posix(noun),
            quoted_posix(&names(verbs))
        ));
    }
    out.push_str(
        "        esac\n    fi\n    COMPREPLY=()\n    for word in $offered; do\n        if [[ $word == \"$cur\"* ]]; then\n            COMPREPLY+=(\"$word\")\n        fi\n    done\n}\n",
    );
    out.push_str(&format!(
        "complete -F _a2m_complete {PROGRAM} {SHORT_NAME}\n"
    ));
    out
}

/// A zsh array of `word:description` items.
fn zsh_items(words: &[(String, String)]) -> String {
    words
        .iter()
        .map(|(word, about)| quoted_posix(&format!("{word}:{about}")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn zsh(words: &Words) -> String {
    let mut out = format!(
        "{}_a2m_complete() {{\n    local -a offered\n",
        header("zsh", "#", "source <(anything2mcp completion zsh)")
    );
    out.push_str(&posix_words(2, "CURRENT", "${words[i]}"));
    out.push_str("    if [[ $PREFIX == -* ]]; then\n");
    out.push_str(&format!(
        "        offered=( {} )\n",
        zsh_items(&words.globals)
    ));
    out.push_str("        case \"$noun $verb\" in\n");
    for (command, flags) in words.flags.iter().filter(|(c, _)| c.contains(' ')) {
        out.push_str(&format!(
            "            {}) offered+=( {} ) ;;\n",
            quoted_posix(command),
            zsh_items(flags)
        ));
    }
    out.push_str("        esac\n        case $noun in\n");
    for (command, flags) in words.flags.iter().filter(|(c, _)| !c.contains(' ')) {
        out.push_str(&format!(
            "            {}) offered+=( {} ) ;;\n",
            quoted_posix(command),
            zsh_items(flags)
        ));
    }
    out.push_str("        esac\n    elif [[ -z $noun ]]; then\n");
    out.push_str(&format!("        offered=( {} )\n", zsh_items(&words.top)));
    out.push_str("    elif [[ -z $verb ]]; then\n        case $noun in\n");
    for (noun, verbs) in &words.second {
        out.push_str(&format!(
            "            {}) offered=( {} ) ;;\n",
            quoted_posix(noun),
            zsh_items(verbs)
        ));
    }
    out.push_str("        esac\n    fi\n    _describe 'word' offered\n}\n");
    out.push_str(&format!("compdef _a2m_complete {PROGRAM} {SHORT_NAME}\n"));
    out
}

/// One fish `complete` line for a word or a flag, offered where the
/// condition holds.
fn fish_line(condition: Option<&str>, word: &str, about: &str) -> String {
    let what = if let Some(long) = word.strip_prefix("--") {
        format!("-l {long}")
    } else if let Some(short) = word.strip_prefix('-') {
        format!("-s {short}")
    } else {
        format!("-a {}", quoted_fish(word))
    };
    let when = condition.map_or_else(String::new, |condition| {
        format!(" -n {}", quoted_fish(condition))
    });
    format!(
        "complete -c {PROGRAM}{when} {what} -d {}\n",
        quoted_fish(about)
    )
}

/// The functions the fish script's conditions call: the words before the
/// cursor that are not flags, and whether they are, or begin with, the
/// words given.
const FISH_FUNCTIONS: &str =
    "function __a2m_words --description 'The words before the cursor that are not flags'
    set -l given (commandline -opc)
    set -e given[1]
    set -l skip 0
    for word in $given
        if test $skip -eq 1
            set skip 0
            continue
        end
        switch $word
            case VALUES
                set skip 1
            case '-*'
            case '*'
                printf '%s\\n' $word
        end
    end
end
function __a2m_at --description 'Whether the words before the cursor are these'
    set -l plain (__a2m_words)
    test \"$plain\" = \"$argv\"
end
function __a2m_in --description 'Whether the words before the cursor begin with these'
    set -l plain (__a2m_words)
    set -l at 1
    for word in $argv
        test \"$plain[$at]\" = \"$word\"; or return 1
        set at (math $at + 1)
    end
end
";

fn fish(words: &Words) -> String {
    let mut out = header("fish", "#", "anything2mcp completion fish | source");
    out.push_str(&FISH_FUNCTIONS.replace("VALUES", &GLOBAL_VALUES.join(" ")));
    out.push_str(&format!("complete -c {PROGRAM} -f\n"));
    for (word, about) in &words.top {
        out.push_str(&fish_line(Some("__a2m_at"), word, about));
    }
    for (noun, verbs) in &words.second {
        for (verb, about) in verbs {
            out.push_str(&fish_line(Some(&format!("__a2m_at {noun}")), verb, about));
        }
    }
    for (command, flags) in &words.flags {
        for (flag, about) in flags {
            out.push_str(&fish_line(
                Some(&format!("__a2m_in {command}")),
                flag,
                about,
            ));
        }
    }
    for (flag, about) in &words.globals {
        out.push_str(&fish_line(None, flag, about));
    }
    out.push_str(&format!("complete -c {SHORT_NAME} -w {PROGRAM}\n"));
    out
}

/// A PowerShell ordered table of words and what each is, on one line.
fn powershell_table(words: &[(String, String)]) -> String {
    let pairs: Vec<String> = words
        .iter()
        .map(|(word, about)| {
            let about = if about.is_empty() { word } else { about };
            format!("{} = {}", quoted_powershell(word), quoted_powershell(about))
        })
        .collect();
    format!("[ordered]@{{ {} }}", pairs.join("; "))
}

fn powershell(words: &Words) -> String {
    let mut out = header(
        "PowerShell",
        "#",
        "anything2mcp completion powershell | Out-String | Invoke-Expression",
    );
    let names = [
        PROGRAM.to_owned(),
        SHORT_NAME.to_owned(),
        format!("{PROGRAM}.exe"),
        format!("{SHORT_NAME}.exe"),
    ]
    .map(|name| quoted_powershell(&name))
    .join(", ");
    out.push_str(&format!(
        "Register-ArgumentCompleter -Native -CommandName {names} -ScriptBlock {{\n    param($wordToComplete, $commandAst, $cursorPosition)\n"
    ));
    out.push_str(&format!("    $top = {}\n", powershell_table(&words.top)));
    out.push_str("    $second = @{\n");
    for (noun, verbs) in &words.second {
        out.push_str(&format!(
            "        {} = {}\n",
            quoted_powershell(noun),
            powershell_table(verbs)
        ));
    }
    out.push_str("    }\n    $flags = @{\n");
    for (command, flags) in &words.flags {
        out.push_str(&format!(
            "        {} = {}\n",
            quoted_powershell(command),
            powershell_table(flags)
        ));
    }
    out.push_str("    }\n");
    out.push_str(&format!(
        "    $globals = {}\n",
        powershell_table(&words.globals)
    ));
    let values = GLOBAL_VALUES
        .iter()
        .map(|value| quoted_powershell(value))
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!(
        "    $values = @({values})
    $plain = @()
    $skip = $false
    foreach ($element in @($commandAst.CommandElements | Select-Object -Skip 1)) {{
        if ($element.Extent.EndOffset -ge $cursorPosition) {{ break }}
        $text = $element.ToString()
        if ($skip) {{ $skip = $false; continue }}
        if ($values -contains $text) {{ $skip = $true; continue }}
        if ($text.StartsWith('-')) {{ continue }}
        $plain += $text
    }}
    $offered = [ordered]@{{}}
    $from = @()
    if ($wordToComplete.StartsWith('-')) {{
        $from += $globals
        $both = ($plain | Select-Object -First 2) -join ' '
        if ($flags.Contains($both)) {{ $from += $flags[$both] }}
        if ($plain.Count -ge 1 -and $flags.Contains($plain[0])) {{ $from += $flags[$plain[0]] }}
    }} elseif ($plain.Count -eq 0) {{
        $from += $top
    }} elseif ($plain.Count -eq 1 -and $second.Contains($plain[0])) {{
        $from += $second[$plain[0]]
    }}
    foreach ($table in $from) {{
        foreach ($pair in $table.GetEnumerator()) {{ $offered[$pair.Key] = $pair.Value }}
    }}
    foreach ($pair in $offered.GetEnumerator()) {{
        if ($pair.Key.StartsWith($wordToComplete)) {{
            [System.Management.Automation.CompletionResult]::new($pair.Key, $pair.Key, 'ParameterValue', $pair.Value)
        }}
    }}
}}
"
    ));
    out
}

/// The script for `shell`, made from `tree`. `None` for a shell this
/// command does not know.
pub(crate) fn script(tree: &Tree, shell: &str) -> Option<String> {
    let words = Words::of(tree);
    match shell {
        "bash" => Some(bash(&words)),
        "zsh" => Some(zsh(&words)),
        "fish" => Some(fish(&words)),
        "powershell" => Some(powershell(&words)),
        _ => None,
    }
}

/// `completion <shell>`: prints the script for the shell named, as it is,
/// whatever the output is. It starts nothing and reads nothing but the
/// command tree.
pub(crate) fn run(tree: &Tree, rest: &[String], json: bool) -> Finished {
    let usage = format!(
        "Run `{PROGRAM} {COMPLETION_COMMAND} <shell>`, with one of: {}.",
        SHELLS.join(", ")
    );
    let refused = |message: String| {
        stopped(
            tree,
            COMPLETION_COMMAND,
            &Failure::new(
                FailureClass::Input,
                codes::SCHEMA_INVALID,
                message,
                usage.clone(),
            ),
            ExitClass::Malformed,
            json,
        )
    };
    let [shell] = rest else {
        return refused(format!(
            "`{PROGRAM} {COMPLETION_COMMAND}` takes one word, the shell's name."
        ));
    };
    match script(tree, shell) {
        Some(script) => Finished {
            code: 0,
            stdout: script,
            stderr: String::new(),
        },
        None => refused(format!(
            "`{}` is not a shell this command makes a script for.",
            super::shown(shell)
        )),
    }
}
