/*

    Modified version of the enigo crate tokenizer

*/
#[derive(Debug, Clone, Copy)]
pub enum Key {
    Shift,
    Control,
    Alt,
    Command,
}

#[derive(Debug, Clone, Copy)]
pub enum Button {
    Left,
    Middle,
    Right,
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
}

#[derive(Debug, Clone)]
pub enum Token {
    Sequence(String),
    Unicode(String),
    KeyUp(Key),
    KeyDown(Key),
    MouseUp(Button),
    MouseDown(Button),
    Click(Button),
}

#[derive(Debug, Clone, Default)]
pub struct StateToken {
    pub down: Vec<Token>,
    pub repeat: Vec<Token>,
    pub up: Vec<Token>,
}

pub fn tokenize(input: String) -> StateToken {
    let mut is_unicode = false;
    let mut has_repeat = false;
    let mut has_wait_up = false;
    let mut state_token = StateToken {
        down: vec![],
        repeat: vec![],
        up: vec![],
    };
    let mut token_vec = vec![];
    let mut buffer = String::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '{' => match chars.next() {
                Some('{') => buffer.push('{'),
                Some(mut c) => {
                    flush(&mut token_vec, &mut buffer, is_unicode);

                    let mut tag = String::new();

                    loop {
                        tag.push(c);
                        match chars.next() {
                            Some('{') => match chars.peek() {
                                Some(&'{') => {
                                    chars.next();
                                    c = '{'
                                }
                                _ => {}
                            },
                            Some('}') => match chars.peek() {
                                Some(&'}') => {
                                    chars.next();
                                    c = '}'
                                }
                                _ => break,
                            },
                            Some(new) => c = new,
                            None => {}
                        }
                    }

                    match &*tag {
                        "REPEAT" => {
                            has_repeat = true;
                            state_token.down = token_vec;
                            token_vec = vec![];
                        }
                        "WAIT_UP" => {
                            has_wait_up = true;

                            if has_repeat {
                                state_token.repeat = token_vec.clone();
                                token_vec.clear();
                            } else {
                                state_token.down = token_vec.clone();
                                token_vec.clear();
                            }
                        }
                        "+UNICODE" => is_unicode = true,
                        "-UNICODE" => is_unicode = false,
                        "+SHIFT" => token_vec.push(Token::KeyDown(Key::Shift)),
                        "-SHIFT" => token_vec.push(Token::KeyUp(Key::Shift)),
                        "+CTRL" => token_vec.push(Token::KeyDown(Key::Control)),
                        "-CTRL" => token_vec.push(Token::KeyUp(Key::Control)),
                        "+META" => token_vec.push(Token::KeyDown(Key::Command)),
                        "-META" => token_vec.push(Token::KeyUp(Key::Command)),
                        "+ALT" => token_vec.push(Token::KeyDown(Key::Alt)),
                        "-ALT" => token_vec.push(Token::KeyUp(Key::Alt)),
                        "+LEFT" => token_vec.push(Token::MouseDown(Button::Left)),
                        "-LEFT" => token_vec.push(Token::MouseUp(Button::Left)),
                        "LEFT" => token_vec.push(Token::Click(Button::Left)),
                        "+MIDDLE" => token_vec.push(Token::MouseDown(Button::Middle)),
                        "-MIDDLE" => token_vec.push(Token::MouseUp(Button::Middle)),
                        "MIDDLE" => token_vec.push(Token::Click(Button::Middle)),
                        "+RIGHT" => token_vec.push(Token::MouseDown(Button::Right)),
                        "-RIGHT" => token_vec.push(Token::MouseUp(Button::Right)),
                        "RIGHT" => token_vec.push(Token::Click(Button::Right)),
                        "SCROLL_UP" => token_vec.push(Token::Click(Button::ScrollUp)),
                        "SCROLL_DOWN" => token_vec.push(Token::Click(Button::ScrollDown)),
                        "SCROLL_LEFT" => token_vec.push(Token::Click(Button::ScrollLeft)),
                        "SCROLL_RIGHT" => token_vec.push(Token::Click(Button::ScrollRight)),
                        _ => {}
                    }
                }
                None => {}
            },
            '}' => {
                if let Some('}') = chars.next() {
                    buffer.push('}')
                }
            }
            _ => buffer.push(c),
        }
    }

    flush(&mut token_vec, &mut buffer, is_unicode);

    if has_wait_up {
        state_token.up = token_vec;
    } else if has_repeat {
        state_token.repeat = token_vec;
    } else {
        state_token.down = token_vec;
    }

    state_token
}

fn flush(tokens: &mut Vec<Token>, buffer: &mut String, is_unicode: bool) {
    if !buffer.is_empty() {
        if is_unicode {
            tokens.push(Token::Unicode(buffer.clone()));
        } else {
            tokens.push(Token::Sequence(buffer.clone()));
        }
    }

    buffer.clear();
}
