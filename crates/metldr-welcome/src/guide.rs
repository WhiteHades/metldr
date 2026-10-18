#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Category {
    ChromeAi,
    Ollama,
    WebGpu,
    Pdf,
    Gmail,
    General,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Faq {
    pub category: Category,
    pub question: &'static str,
    pub answer: &'static str,
    pub help_url: Option<&'static str>,
}

pub const CATEGORIES: [(Category, &str, &str); 6] = [
    (Category::ChromeAi, "chrome-ai", "Chrome AI"),
    (Category::Ollama, "ollama", "Ollama"),
    (Category::WebGpu, "webgpu", "WebGPU and search"),
    (Category::Pdf, "pdf", "PDFs"),
    (Category::Gmail, "gmail", "Gmail"),
    (Category::General, "general", "General"),
];

pub const FAQS: [Faq; 15] = [
    Faq {
        category: Category::ChromeAi,
        question: "Chrome AI is unavailable or not ready",
        answer: "Chrome AI availability depends on Chrome and the device. Check the extension's AI status, then consult Chrome's built-in AI requirements.",
        help_url: Some("https://developer.chrome.com/docs/ai/built-in"),
    },
    Faq {
        category: Category::ChromeAi,
        question: "Chrome AI setup is not completing",
        answer: "Use Check to read Chrome AI availability. If a download is available, choose Download model to start it. Cancel stops this setup request, but Chrome may continue a download shared with other pages.",
        help_url: Some("https://developer.chrome.com/docs/ai/built-in"),
    },
    Faq {
        category: Category::ChromeAi,
        question: "Chrome AI responses are slow",
        answer: "Response time can vary. Check whether the extension reports Chrome AI as ready and try again after setup finishes.",
        help_url: None,
    },
    Faq {
        category: Category::Ollama,
        question: "Ollama is not connecting",
        answer: "Start Ollama, then choose Check. This setup uses 127.0.0.1:11434. Ollama may also require this extension's origin in OLLAMA_ORIGINS. Follow its official platform instructions and allow only the extension you intend to use.",
        help_url: Some("https://docs.ollama.com/faq"),
    },
    Faq {
        category: Category::Ollama,
        question: "Ollama has no models available",
        answer: "The extension lists models available from your Ollama service. Install a model using Ollama's own instructions, then refresh the extension's model list.",
        help_url: Some("https://docs.ollama.com/quickstart"),
    },
    Faq {
        category: Category::Ollama,
        question: "Ollama responses are slow",
        answer: "Response time depends on the model and the system. Check Ollama's status and model documentation for troubleshooting guidance.",
        help_url: Some("https://docs.ollama.com/faq"),
    },
    Faq {
        category: Category::WebGpu,
        question: "WebGPU is unavailable",
        answer: "WebGPU support depends on the browser and device. The guide does not recommend changing browser flags. Check the extension's status and browser support information.",
        help_url: Some("https://developer.chrome.com/docs/web-platform/webgpu"),
    },
    Faq {
        category: Category::WebGpu,
        question: "Search does not find relevant results",
        answer: "Summarize the page first, wait for indexing to finish, then search with terms from it. Indexing progress appears in the side panel.",
        help_url: None,
    },
    Faq {
        category: Category::Pdf,
        question: "A local PDF will not open",
        answer: "Chrome restricts extension access to local files. Use the extension's PDF file picker to select a local PDF.",
        help_url: None,
    },
    Faq {
        category: Category::Pdf,
        question: "A PDF summary is incomplete",
        answer: "PDF text extraction can vary with document structure. Review the original document before using the summary.",
        help_url: None,
    },
    Faq {
        category: Category::Gmail,
        question: "The Gmail summary button is missing",
        answer: "The summary control is shown for an open email thread. Refresh Gmail and check that the extension has access to the page.",
        help_url: None,
    },
    Faq {
        category: Category::Gmail,
        question: "A Gmail summary is inaccurate",
        answer: "Summaries are generated from the email content available to the extension. Review the original thread before acting on details in a summary.",
        help_url: None,
    },
    Faq {
        category: Category::General,
        question: "Is my data sent to servers?",
        answer: "The connection check sends no prompt. Summaries and chat in the current extension use Chrome AI or the configured Ollama service. Ollama can use local or cloud models. Review its selected model and settings before sharing private content.",
        help_url: Some("https://docs.ollama.com/faq"),
    },
    Faq {
        category: Category::General,
        question: "The extension is not loading",
        answer: "Reload the extension from Chrome's extensions page. If the issue continues, inspect the extension console for an error.",
        help_url: None,
    },
    Faq {
        category: Category::General,
        question: "How do I update the extension?",
        answer: "For an installed store version, use Chrome's extension update flow. For a development build, rebuild from the project and reload it from Chrome's extensions page.",
        help_url: None,
    },
];

pub fn filter_faqs(category: Option<Category>, query: &str) -> impl Iterator<Item = &'static Faq> {
    let query = query.trim().to_lowercase();
    FAQS.iter().filter(move |faq| {
        category.is_none_or(|selected| faq.category == selected)
            && (query.is_empty()
                || faq.question.to_lowercase().contains(&query)
                || faq.answer.to_lowercase().contains(&query))
    })
}

pub fn render_html() -> String {
    use std::fmt::Write;
    let mut html = format!(
        "<p id=\"faq-count\" role=\"status\">{} answers</p>\n",
        FAQS.len()
    );
    for (index, faq) in FAQS.iter().enumerate() {
        write!(html, "<details name=\"welcome-faq\" id=\"faq-{index}\"><summary>{}<svg viewBox=\"0 0 24 24\" aria-hidden=\"true\"><path d=\"m9 6 6 6-6 6\"/></svg></summary><div><p>{}</p>", escape(faq.question), escape(faq.answer)).unwrap();
        if let Some(url) = faq.help_url {
            write!(html, "<p><a href=\"{}\" target=\"_blank\" rel=\"noreferrer\">Read the official guide</a></p>", escape(url)).unwrap();
        }
        html.push_str("</div></details>\n");
    }
    html
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::{Category, filter_faqs};

    #[test]
    fn filters_by_category_and_case_insensitive_question_or_answer() {
        let results: Vec<_> = filter_faqs(Some(Category::Pdf), "DOCUMENT STRUCTURE").collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].question, "A PDF summary is incomplete");
        assert_eq!(filter_faqs(Some(Category::Pdf), "INCOMPLETE").count(), 1);
        assert!(
            filter_faqs(Some(Category::Pdf), "no matching topic")
                .next()
                .is_none()
        );
    }
}
