use crate::{Error, Result};

/// The recovered implementation used shlex.split(comments=False). Preserve
/// spaces/quotes and Windows path backslashes without invoking a shell.
pub(super) fn split(line: &str) -> Result<Vec<String>> {
    let mut chars = line.chars().peekable();
    let mut quote = None;
    let mut token = String::new();
    let mut started = false;
    let mut out = Vec::new();
    while let Some(c) = chars.next() {
        match quote {
            Some('\'') => {
                if c == '\'' {
                    quote = None;
                } else {
                    token.push(c);
                }
            }
            Some('"') => match c {
                '"' => quote = None,
                '\\' => {
                    let next = chars.next().ok_or(Error::Media("unfinished CUE escape"))?;
                    if next != '"' && next != '\\' {
                        token.push('\\');
                    }
                    token.push(next);
                }
                _ => token.push(c),
            },
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    started = true;
                }
                '\\' => {
                    token.push(chars.next().ok_or(Error::Media("unfinished CUE escape"))?);
                    started = true;
                }
                ' ' | '\t' | '\r' | '\n' => {
                    if started {
                        out.push(std::mem::take(&mut token));
                        started = false;
                    }
                }
                _ => {
                    token.push(c);
                    started = true;
                }
            },
            _ => unreachable!("only single and double quotes"),
        }
    }
    if quote.is_some() {
        return Err(Error::Media("unclosed CUE quote"));
    }
    if started {
        out.push(token);
    }
    Ok(out)
}
