use regex::Regex;

pub fn load_test_regex(input_for_regex: Option<String>, regex_to_use: Option<String>) {
    // Either we got an input to use the regex on, or we create a random input
    let input = input_for_regex.unwrap_or_else(|| generate_random_text(1024));

    // Either we got a regex to use, or we create a random one
    let regex_str = regex_to_use.unwrap_or_else(|| {
        let pattern_length = 12usize;
        // at most, we will use 3 bytes for each pattern entry
        let random_bytes =
            plaid_stl::plaid::random::fetch_random_bytes(3 * pattern_length as u16).unwrap();
        random_regex_from_bytes(&random_bytes, pattern_length)
    });
    let regex = Regex::new(&regex_str).unwrap();

    // Finally, apply the regex on the input
    let _ = regex.is_match(&input);
}

fn generate_random_text(length: usize) -> String {
    let mut random_string = Vec::with_capacity(length);

    while random_string.len() < length {
        let random_bytes = plaid_stl::plaid::random::fetch_random_bytes(512).unwrap();
        random_string.extend(
            random_bytes
                .into_iter()
                .filter(|b| b.is_ascii())
                .take(length - random_string.len()),
        );
    }

    String::from_utf8(random_string).unwrap()
}

fn get_byte(bytes: &[u8], index: &mut usize) -> u8 {
    let byte = bytes[*index % bytes.len()];
    *index += 1;
    byte
}

fn random_regex_from_bytes(bytes: &[u8], pattern_length: usize) -> String {
    let char_classes = [
        "\\d", "\\w", "\\s", ".", "[a-z]", "[A-Z]", "[0-9]", "[aeiou]",
    ];
    let quantifiers = ["", "*", "+", "?", "{1,3}"];

    let mut index = 0;
    let mut pattern = String::new();

    for _ in 0..pattern_length {
        let choice = get_byte(bytes, &mut index) % 3;

        let fragment = match choice {
            0 => {
                let class_idx = get_byte(bytes, &mut index) as usize % char_classes.len();
                let quant_idx = get_byte(bytes, &mut index) as usize % quantifiers.len();
                format!("{}{}", char_classes[class_idx], quantifiers[quant_idx])
            }
            1 => {
                let ch = (b'a' + get_byte(bytes, &mut index) % 26) as char;
                let quant_idx = get_byte(bytes, &mut index) as usize % quantifiers.len();
                format!("{}{}", ch, quantifiers[quant_idx])
            }
            2 => {
                let group_char = (b'a' + get_byte(bytes, &mut index) % 26) as char;
                format!("({})", group_char)
            }
            _ => unreachable!(),
        };

        pattern.push_str(&fragment);
    }

    pattern
}
