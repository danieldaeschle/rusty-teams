#[derive(Clone)]
pub enum Row {
    DaySeparator(String),
    Message(ChatMessage),
}

#[derive(Clone)]
pub struct ChatMessage {
    pub author: String,
    pub time: String,
    pub markdown: String,
}

const AUTHORS: [&str; 6] = ["Author A", "Author B", "Author C", "Author D", "Author E", "Author F"];
const WORDS: [&str; 14] = [
    "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet",
    "kilo", "lima", "mike", "november",
];

struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % bound
    }
}

fn sentence(random: &mut Lcg, word_count: u64) -> String {
    (0..word_count)
        .map(|_| WORDS[random.next(WORDS.len() as u64) as usize])
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn synthetic_rows(message_count: usize) -> Vec<Row> {
    let mut random = Lcg(42);
    let mut rows = Vec::with_capacity(message_count + message_count / 50 + 1);
    let mut day = 0;
    for message_index in 0..message_count {
        if message_index % 50 == 0 {
            rows.push(Row::DaySeparator(format!("Day {}", day + 1)));
            day += 1;
        }
        let line_count = 1 + random.next(15);
        let mut lines: Vec<String> = Vec::new();
        for line_index in 0..line_count {
            let line = match random.next(8) {
                0 => format!("**{}** {}", sentence(&mut random, 2), sentence(&mut random, 8)),
                1 => format!("{} [link {}](https://example.com/{})", sentence(&mut random, 5), message_index, line_index),
                _ => {
                    let word_count = 4 + random.next(24);
                    sentence(&mut random, word_count)
                }
            };
            lines.push(line);
        }
        let mut markdown = lines.join("\n\n");
        if random.next(6) == 0 {
            markdown.push_str(&format!(
                "\n\n```rust\nfn handler_{message_index}(input: &str) -> usize {{\n    input.len() + {message_index}\n}}\n```"
            ));
        }
        rows.push(Row::Message(ChatMessage {
            author: AUTHORS[random.next(AUTHORS.len() as u64) as usize].to_string(),
            time: format!("{:02}:{:02}", 8 + message_index % 10, (message_index * 7) % 60),
            markdown,
        }));
    }
    rows
}
