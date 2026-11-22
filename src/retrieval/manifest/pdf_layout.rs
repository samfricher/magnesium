use std::collections::{HashMap, HashSet};

use pdf_extract::{Document, MediaBox, OutputDev, OutputError, Transform};
use serde::{Deserialize, Serialize};

const TOP_SHARE: f64 = 0.34;
const KEPT_TOP: f64 = 0.4;
const MAX_LINES_PER_PAGE: usize = 12;
const MAX_LINE_CHARS: usize = 200;
const MAX_TITLE_WORDS: usize = 14;
const MAX_OUTLINE_ENTRIES: usize = 4_096;
const MIN_REPEATED_PAGES: usize = 3;
const REPEATED_PAGE_PERCENT: usize = 50;
const USABLE_PAGE_PERCENT: usize = 50;
const TITLE_OVER_BODY: f64 = 1.15;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PdfLayout {
    #[serde(default)]
    pub(crate) outline: Vec<OutlineEntry>,
    #[serde(default)]
    pub(crate) tops: Vec<Vec<TopLine>>,
    #[serde(default)]
    pub(crate) body_size: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OutlineEntry {
    pub(crate) level: usize,
    pub(crate) title: String,
    pub(crate) page: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TopLine {
    pub(crate) text: String,
    pub(crate) size: f64,
    pub(crate) top: f64,
}

pub(crate) fn read_pdf(bytes: &[u8]) -> Result<(Vec<String>, PdfLayout), OutputError> {
    let mut document = Document::load_mem(bytes)?;
    if document.is_encrypted() {
        document.decrypt("")?;
    }
    let mut pages = Vec::new();
    let mut tops = Vec::new();
    let mut sizes: HashMap<i64, usize> = HashMap::new();
    let mut page_num = 1;
    loop {
        let mut page = PageText::default();
        if pdf_extract::output_doc_page(&document, &mut page, page_num).is_err() {
            break;
        }
        page.close_line();
        for (size, chars) in page.sizes {
            *sizes.entry(size).or_insert(0) += chars;
        }
        tops.push(page.tops);
        pages.push(page.text);
        page_num += 1;
    }
    let body_size = weighted_mode(sizes);
    let outline = document
        .get_toc()
        .map(|toc| {
            toc.toc
                .into_iter()
                .take(MAX_OUTLINE_ENTRIES)
                .filter_map(|entry| {
                    let title = clean(&entry.title);
                    let page = u32::try_from(entry.page).ok()?;
                    (!title.is_empty() && page >= 1).then_some(OutlineEntry {
                        level: entry.level,
                        title,
                        page,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((
        pages,
        PdfLayout {
            outline,
            tops,
            body_size,
        },
    ))
}

#[derive(Default)]
struct PageText {
    text: String,
    last_end: f64,
    last_y: f64,
    first_char: bool,
    height: f64,
    ury: f64,
    line: String,
    line_sizes: HashMap<i64, usize>,
    line_top: Option<f64>,
    tops: Vec<TopLine>,
    sizes: HashMap<i64, usize>,
}

impl PageText {
    fn close_line(&mut self) {
        let text = clean(&self.line);
        let line_sizes = std::mem::take(&mut self.line_sizes);
        for (size, chars) in &line_sizes {
            *self.sizes.entry(*size).or_insert(0) += chars;
        }
        if let Some(top) = self.line_top
            && let Some(size) = weighted_mode(line_sizes)
            && !text.is_empty()
            && top <= KEPT_TOP
            && self.tops.len() < MAX_LINES_PER_PAGE
        {
            self.tops.push(TopLine {
                text: text.chars().take(MAX_LINE_CHARS).collect(),
                size,
                top,
            });
        }
        self.line.clear();
        self.line_top = None;
    }
}

impl OutputDev for PageText {
    fn begin_page(
        &mut self,
        _page_num: u32,
        media_box: &MediaBox,
        _: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        self.last_end = 100000.;
        self.height = media_box.ury - media_box.lly;
        self.ury = media_box.ury;
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn output_character(
        &mut self,
        trm: &Transform,
        width: f64,
        _spacing: f64,
        font_size: f64,
        char: &str,
    ) -> Result<(), OutputError> {
        let x = trm.m31;
        let y = self.height - trm.m32;
        let scaled_x = font_size * trm.m11 + font_size * trm.m21;
        let scaled_y = font_size * trm.m12 + font_size * trm.m22;
        let transformed_font_size = (scaled_x * scaled_y).sqrt();
        if self.first_char {
            if (y - self.last_y).abs() > transformed_font_size * 1.5 {
                self.text.push('\n');
                self.close_line();
            }
            if x < self.last_end && (y - self.last_y).abs() > transformed_font_size * 0.5 {
                self.text.push('\n');
                self.close_line();
            }
            if x > self.last_end + transformed_font_size * 0.1 {
                self.text.push(' ');
                self.line.push(' ');
            }
        }
        self.text.push_str(char);
        self.line.push_str(char);
        let size = (scaled_x * scaled_y).abs().sqrt();
        if size.is_finite() && size > 0.0 && !char.trim().is_empty() {
            *self.line_sizes.entry(size_key(size)).or_insert(0) += 1;
        }
        if self.line_top.is_none() && self.height > 0.0 {
            let top = (self.ury - trm.m32) / self.height;
            self.line_top = top.is_finite().then_some(top);
        }
        self.first_char = false;
        self.last_y = y;
        self.last_end = x + width * transformed_font_size;
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        self.first_char = true;
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
}

fn size_key(size: f64) -> i64 {
    (size * 2.0).round() as i64
}

fn weighted_mode(weights: HashMap<i64, usize>) -> Option<f64> {
    weights
        .into_iter()
        .max_by_key(|(size, weight)| (*weight, -*size))
        .map(|(size, _)| size as f64 / 2.0)
}

fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn repeat_key(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_ascii_digit())
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn page_titles(layout: &PdfLayout, page_count: usize) -> Vec<Option<String>> {
    let pages = page_count.min(layout.tops.len());
    let mut seen: HashMap<String, usize> = HashMap::new();
    for page in layout.tops.iter().take(pages) {
        let keys = page
            .iter()
            .map(|line| repeat_key(&line.text))
            .collect::<HashSet<_>>();
        for key in keys {
            *seen.entry(key).or_insert(0) += 1;
        }
    }
    let repeated = |line: &TopLine| {
        let count = seen.get(&repeat_key(&line.text)).copied().unwrap_or(0);
        count >= MIN_REPEATED_PAGES && count * 100 >= pages * REPEATED_PAGE_PERCENT
    };
    let body = layout.body_size.or_else(|| {
        let mut weights: HashMap<i64, usize> = HashMap::new();
        for line in layout.tops.iter().take(pages).flatten() {
            if !repeated(line) && line.size.is_finite() {
                *weights.entry(size_key(line.size)).or_insert(0) += line.text.chars().count();
            }
        }
        weighted_mode(weights)
    });
    let own = layout
        .tops
        .iter()
        .take(pages)
        .map(|page| {
            page.iter()
                .filter(|line| line.top <= TOP_SHARE && plausible_title(&line.text))
                .filter(|line| !repeated(line))
                .filter(|line| body.is_some_and(|body| line.size >= body * TITLE_OVER_BODY))
                .fold(None::<&TopLine>, |best, line| match best {
                    Some(best) if best.size >= line.size => Some(best),
                    _ => Some(line),
                })
                .map(|line| line.text.clone())
        })
        .collect::<Vec<_>>();
    let with_text = layout
        .tops
        .iter()
        .take(pages)
        .filter(|page| !page.is_empty())
        .count();
    let titled = own.iter().filter(|title| title.is_some()).count();
    let own_usable = with_text > 0 && titled * 100 >= with_text * USABLE_PAGE_PERCENT;
    let mut outline = layout
        .outline
        .iter()
        .filter(|entry| entry.page as usize <= page_count)
        .collect::<Vec<_>>();
    outline.sort_by_key(|entry| entry.page);
    let mut stack: Vec<&OutlineEntry> = Vec::new();
    let mut next = 0;
    (0..page_count)
        .map(|index| {
            let page = u32::try_from(index + 1).unwrap_or(u32::MAX);
            while next < outline.len() && outline[next].page <= page {
                let entry = outline[next];
                while stack.last().is_some_and(|top| top.level >= entry.level) {
                    stack.pop();
                }
                stack.push(entry);
                next += 1;
            }
            let mut path = stack
                .iter()
                .map(|entry| entry.title.clone())
                .collect::<Vec<_>>();
            if own_usable
                && let Some(Some(title)) = own.get(index)
                && path
                    .last()
                    .is_none_or(|last| repeat_key(last) != repeat_key(title))
            {
                path.push(title.clone());
            }
            (!path.is_empty()).then(|| path.join(" > "))
        })
        .collect()
}

fn plausible_title(text: &str) -> bool {
    let words = text.split_whitespace().count();
    (1..=MAX_TITLE_WORDS).contains(&words)
        && text.chars().any(char::is_alphabetic)
        && !text.ends_with(',')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, size: f64, top: f64) -> TopLine {
        TopLine {
            text: text.to_owned(),
            size,
            top,
        }
    }

    fn slide(title: &str) -> Vec<TopLine> {
        vec![
            line("Chapter 8. Estimation Confidence Intervals", 9.0, 0.02),
            line("Selecting the Sample Size", 9.0, 0.05),
            line(title, 22.0, 0.16),
            line("Suppose that the study involved", 16.0, 0.36),
        ]
    }

    #[test]
    fn the_largest_line_near_the_top_that_is_not_the_running_header_names_the_page() {
        let layout = PdfLayout {
            outline: vec![
                OutlineEntry {
                    level: 1,
                    title: "Chapter 8. Estimation".into(),
                    page: 1,
                },
                OutlineEntry {
                    level: 2,
                    title: "Confidence Intervals".into(),
                    page: 3,
                },
            ],
            body_size: Some(16.0),
            tops: vec![
                slide("Toy Problem"),
                slide("Definition 8.2"),
                slide("Pivotal quantity"),
                slide("Exercise 8.59"),
                slide("Exercise 8.59 (cont.)"),
            ],
        };
        assert_eq!(
            page_titles(&layout, 5),
            [
                Some("Chapter 8. Estimation > Toy Problem".to_owned()),
                Some("Chapter 8. Estimation > Definition 8.2".to_owned()),
                Some("Chapter 8. Estimation > Confidence Intervals > Pivotal quantity".to_owned()),
                Some("Chapter 8. Estimation > Confidence Intervals > Exercise 8.59".to_owned()),
                Some(
                    "Chapter 8. Estimation > Confidence Intervals > Exercise 8.59 (cont.)"
                        .to_owned()
                ),
            ]
        );
    }

    #[test]
    fn page_counters_count_as_repeated_once_their_digits_are_ignored() {
        let tops = (1..=6)
            .map(|page| {
                vec![
                    line(&format!("Page {page} of 6"), 30.0, 0.01),
                    line(
                        &format!("Topic {}", ["a", "b", "c", "d", "e", "f"][page - 1]),
                        20.0,
                        0.1,
                    ),
                    line("A line of body text under the topic", 12.0, 0.2),
                ]
            })
            .collect();
        let titles = page_titles(
            &PdfLayout {
                outline: Vec::new(),
                tops,
                body_size: Some(12.0),
            },
            6,
        );
        assert_eq!(titles[0].as_deref(), Some("Topic a"));
        assert_eq!(titles[5].as_deref(), Some("Topic f"));
    }

    #[test]
    fn body_text_is_not_a_title_and_a_file_with_too_few_titles_keeps_none() {
        let sentence = "This is a long sentence of body text that runs on well past any slide title, as prose does";
        let tops = (0..4)
            .map(|index| vec![line(&format!("{sentence} {index}"), 10.0, 0.05)])
            .collect();
        let titles = page_titles(
            &PdfLayout {
                outline: Vec::new(),
                tops,
                body_size: None,
            },
            4,
        );
        assert!(titles.iter().all(Option::is_none), "{titles:?}");
    }

    #[test]
    fn a_line_no_larger_than_the_body_text_is_not_a_title() {
        let tops = (0..6)
            .map(|index| {
                vec![
                    line(&format!("Short line {index}"), 10.0, 0.05),
                    line(
                        "and the paragraph carries on below it in the same size",
                        10.0,
                        0.08,
                    ),
                ]
            })
            .collect();
        let titles = page_titles(
            &PdfLayout {
                outline: Vec::new(),
                tops,
                body_size: None,
            },
            6,
        );
        assert!(titles.iter().all(Option::is_none), "{titles:?}");
    }

    #[test]
    fn an_outline_alone_names_pages_when_the_pages_give_no_titles() {
        let layout = PdfLayout {
            outline: vec![
                OutlineEntry {
                    level: 1,
                    title: "8 Estimation".into(),
                    page: 2,
                },
                OutlineEntry {
                    level: 2,
                    title: "8.3 Common unbiased estimators".into(),
                    page: 3,
                },
                OutlineEntry {
                    level: 1,
                    title: "9 Properties".into(),
                    page: 4,
                },
            ],
            tops: vec![Vec::new(); 4],
            body_size: None,
        };
        assert_eq!(
            page_titles(&layout, 4),
            [
                None,
                Some("8 Estimation".to_owned()),
                Some("8 Estimation > 8.3 Common unbiased estimators".to_owned()),
                Some("9 Properties".to_owned()),
            ]
        );
    }
}
