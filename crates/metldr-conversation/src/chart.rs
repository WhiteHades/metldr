use serde::{Deserialize, Deserializer, Serialize};

pub const MAX_CHART_BYTES: usize = 262_144;
pub const MAX_CHART_EXPORT_BYTES: usize = 1_048_576;

const MAX_TITLE_UTF16: usize = 300;
const MAX_DESCRIPTION_UTF16: usize = 1_000;
const MAX_LABEL_UTF16: usize = 200;
const MAX_POINTS: usize = 200;
const CHART_JS: &str = include_str!("../assets/chart.umd.min.js");
const COLORS: [&str; 7] = [
    "#3b82f6", "#8b5cf6", "#ec4899", "#f59e0b", "#10b981", "#06b6d4", "#f97316",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartKind {
    Bar,
    Pie,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChartDatum {
    pub label: String,
    pub value: serde_json::Number,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Chart {
    kind: ChartKind,
    title: String,
    description: String,
    data: Vec<ChartDatum>,
}

impl Chart {
    pub fn new(
        kind: ChartKind,
        title: String,
        description: String,
        data: Vec<ChartDatum>,
    ) -> Result<Self, String> {
        if title.encode_utf16().count() > MAX_TITLE_UTF16 {
            return Err("chart title exceeds its UTF-16 limit".into());
        }
        if description.encode_utf16().count() > MAX_DESCRIPTION_UTF16 {
            return Err("chart description exceeds its UTF-16 limit".into());
        }
        if data.len() > MAX_POINTS {
            return Err("chart exceeds its point limit".into());
        }

        let mut total = 0.0;
        let mut min = 0.0_f64;
        let mut max = 0.0_f64;
        for point in &data {
            if point.label.encode_utf16().count() > MAX_LABEL_UTF16 {
                return Err("chart label exceeds its UTF-16 limit".into());
            }
            let value = js_number(&point.value)?;
            if kind == ChartKind::Pie && value < 0.0 {
                return Err("pie chart values cannot be negative".into());
            }
            min = min.min(value);
            max = max.max(value);
            if kind == ChartKind::Pie {
                total += value;
            }
            if kind == ChartKind::Pie && !total.is_finite() {
                return Err("chart values have a non-finite total".into());
            }
        }
        if kind == ChartKind::Bar && !(max - min).is_finite() {
            return Err("bar chart span is non-finite".into());
        }

        let chart = Self {
            kind,
            title,
            description,
            data,
        };
        if serde_json::to_vec(&chart)
            .map_err(|error| format!("could not encode chart: {error}"))?
            .len()
            > MAX_CHART_BYTES
        {
            return Err("chart exceeds its canonical JSON byte limit".into());
        }
        Ok(chart)
    }

    pub fn kind(&self) -> ChartKind {
        self.kind
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn data(&self) -> &[ChartDatum] {
        &self.data
    }

    pub fn source(&self) -> &'static str {
        "Model supplied data"
    }

    pub fn html(&self) -> Result<String, String> {
        let labels = script_json(
            &self
                .data
                .iter()
                .map(|point| &point.label)
                .collect::<Vec<_>>(),
        )?;
        let values = script_json(
            &self
                .data
                .iter()
                .map(|point| &point.value)
                .collect::<Vec<_>>(),
        )?;
        let colors = script_json(
            &(0..self.data.len())
                .map(|index| COLORS[index % COLORS.len()])
                .collect::<Vec<_>>(),
        )?;
        let config = match self.kind {
            ChartKind::Bar => format!(
                r#"{{
        type: 'bar',
        data: {{ labels: {labels}, datasets: [{{ data: {values}, backgroundColor: {colors}, borderRadius: 4 }}] }},
        options: chartOptions({{ display: false }}, true)
      }}"#
            ),
            ChartKind::Pie => format!(
                r#"{{
        type: 'pie',
        data: {{ labels: {labels}, datasets: [{{ data: {values}, backgroundColor: {colors} }}] }},
        options: chartOptions({{ position: 'bottom', labels: {{ padding: 16, usePointStyle: true }} }}, false)
      }}"#
            ),
        };
        let accessible_label = escape_html(&format!("{}: {}", self.title, self.description));
        let data_rows = self
            .data
            .iter()
            .map(|point| {
                format!(
                    "<tr><th scope=\"row\">{}</th><td>{}</td></tr>",
                    escape_html(&point.label),
                    point.value
                )
            })
            .collect::<String>();
        let html = format!(
            r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="color-scheme" content="light dark">
  <title>{}</title>
  <style>
    :root {{
      color-scheme: light dark;
      overflow-wrap: break-word;
      --paper: light-dark(oklch(0.99 0 none), oklch(0.22 0 none));
      --ink: light-dark(oklch(0.2 0 none), oklch(0.94 0 none));
      --muted: light-dark(oklch(0.48 0 none), oklch(0.72 0 none));
      --axis: light-dark(oklch(0.45 0 none), oklch(0.72 0 none));
      --grid: light-dark(oklch(0.88 0 none), oklch(0.36 0 none));
      --tooltip: light-dark(oklch(0.24 0 none), oklch(0.94 0 none));
      --tooltip-ink: light-dark(oklch(0.98 0 none), oklch(0.2 0 none));
    }}
    body {{
      --gutter: 24px;
      --content: 640px;
      display: grid;
      grid-template-columns: [full-width-start] minmax(var(--gutter), 1fr) [content-start] min(100% - var(--gutter) * 2, var(--content)) [content-end] minmax(var(--gutter), 1fr) [full-width-end];
      margin: 0;
      color: var(--ink);
      background: var(--paper);
      font-family: system-ui, -apple-system, sans-serif;
    }}
    body > * {{ grid-column: content; }}
    body > script {{ display: none; }}
    h3 {{ font-size: 20px; font-weight: 700; margin-block: 0 4px; }}
    p {{ font-size: 14px; color: var(--muted); margin-block: 0 20px; }}
    .chart-plot {{ position: relative; min-inline-size: 0; }}
    canvas {{ max-block-size: 360px; }}
    details {{ margin-block-start: 16px; color: var(--muted); }}
    summary {{ cursor: pointer; }}
    summary:focus-visible {{ outline: 2px solid var(--ink); outline-offset: 3px; }}
    summary:active {{ color: var(--ink); }}
    table {{ border-collapse: collapse; margin-block-start: 8px; inline-size: 100%; text-align: start; }}
    th, td {{ padding-block: 4px; padding-inline: 8px; border-block-end: 1px solid var(--grid); overflow-wrap: anywhere; }}
    th {{ font-weight: 600; text-align: start; }}
    td {{ text-align: end; font-variant-numeric: tabular-nums; }}
  </style>
</head>
<body>
  <h3>{}</h3>
  <p>{}</p>
  <div class="chart-plot"><canvas id="chart" role="img" aria-label="{}"></canvas></div>
  <details><summary>View chart data</summary><table><caption>Chart data</caption><thead><tr><th scope="col">Label</th><th scope="col">Value</th></tr></thead><tbody>{}</tbody></table></details>
  <p>Source: {}</p>
  <script>{CHART_JS}</script>
  <script>
    const colorProbe = document.createElement('span');
    colorProbe.style.cssText = 'position:absolute;visibility:hidden;color:var(--axis);background-color:var(--grid);border-color:var(--tooltip);outline-color:var(--tooltip-ink)';
    document.body.append(colorProbe);
    const resolvedStyle = getComputedStyle(colorProbe);
    const colorCanvas = document.createElement('canvas');
    colorCanvas.width = colorCanvas.height = 1;
    const colorContext = colorCanvas.getContext('2d', {{ willReadFrequently: true }});
    const rgb = (color) => {{
      colorContext.clearRect(0, 0, 1, 1);
      colorContext.fillStyle = color;
      colorContext.fillRect(0, 0, 1, 1);
      const [r, g, b, a] = colorContext.getImageData(0, 0, 1, 1).data;
      return `rgba(${{r}},${{g}},${{b}},${{a / 255}})`;
    }};
    const chartColors = {{ axis: rgb(resolvedStyle.color), grid: rgb(resolvedStyle.backgroundColor), tooltip: rgb(resolvedStyle.borderColor), tooltipInk: rgb(resolvedStyle.outlineColor) }};
    colorProbe.remove();
    const chartOptions = (legend, bar) => ({{
      responsive: true,
      animation: matchMedia('(prefers-reduced-motion: reduce)').matches ? false : {{ duration: 800, easing: 'easeOutCubic' }},
      plugins: {{
        legend: {{ ...legend, labels: {{ ...legend.labels, color: chartColors.axis }} }},
        tooltip: {{ backgroundColor: chartColors.tooltip, titleColor: chartColors.tooltipInk, bodyColor: chartColors.tooltipInk, cornerRadius: 8, padding: 10 }}
      }},
      ...(bar ? {{ scales: {{ x: {{ grid: {{ display: false }}, ticks: {{ color: chartColors.axis }} }}, y: {{ grid: {{ color: chartColors.grid }}, ticks: {{ color: chartColors.axis }} }} }} }} : {{}})
    }});
    new Chart(document.getElementById('chart'), {config});
  </script>
</body>
</html>"#,
            escape_html(&self.title),
            escape_html(&self.title),
            escape_html(&self.description),
            accessible_label,
            data_rows,
            self.source()
        );
        if html.len() > MAX_CHART_EXPORT_BYTES {
            return Err("chart export exceeds its byte limit".into());
        }
        Ok(html)
    }

    pub fn filename(&self) -> String {
        let slug = self
            .title
            .to_ascii_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let slug = if slug.is_empty() {
            "visualization"
        } else {
            &slug
        };
        format!("{}.html", &slug[..slug.len().min(100)])
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawChart {
    kind: ChartKind,
    title: String,
    description: String,
    data: Vec<ChartDatum>,
}

impl<'de> Deserialize<'de> for Chart {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawChart::deserialize(deserializer)?;
        Self::new(raw.kind, raw.title, raw.description, raw.data).map_err(serde::de::Error::custom)
    }
}

fn js_number(number: &serde_json::Number) -> Result<f64, String> {
    let value = number.as_f64().ok_or("chart value is not a number")?;
    if !value.is_finite() {
        return Err("chart values must be finite".into());
    }
    if let Some(integer) = number.as_u64() {
        if (value as u128) != integer as u128 {
            return Err("chart integer loses precision in JavaScript".into());
        }
    } else if let Some(integer) = number.as_i64() {
        if (value as i128) != integer as i128 {
            return Err("chart integer loses precision in JavaScript".into());
        }
    }
    Ok(value)
}

fn script_json(value: &impl Serialize) -> Result<String, String> {
    let json = serde_json::to_string(value)
        .map_err(|error| format!("could not encode chart data: {error}"))?;
    Ok(json
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029"))
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn datum(label: &str, value: serde_json::Number) -> ChartDatum {
        ChartDatum {
            label: label.into(),
            value,
        }
    }

    fn chart(kind: ChartKind, data: Vec<ChartDatum>) -> Chart {
        Chart::new(kind, "Report".into(), "Summary".into(), data).unwrap()
    }

    #[test]
    fn accepts_negative_bars_empty_and_zero_pies_and_duplicate_blank_labels() {
        assert!(
            chart(ChartKind::Bar, vec![datum("loss", (-3).into())])
                .html()
                .is_ok()
        );
        assert!(chart(ChartKind::Pie, vec![]).html().is_ok());
        assert!(
            chart(ChartKind::Pie, vec![datum("zero", 0.into())])
                .html()
                .is_ok()
        );
        assert!(
            chart(
                ChartKind::Bar,
                vec![
                    datum("", 1.into()),
                    datum("same", 2.into()),
                    datum("same", 3.into())
                ]
            )
            .html()
            .is_ok()
        );
    }

    #[test]
    fn enforces_numeric_precision_and_overflow_boundaries() {
        assert!(
            Chart::new(
                ChartKind::Bar,
                "Report".into(),
                "".into(),
                vec![datum("safe", 9_007_199_254_740_991_u64.into())]
            )
            .is_ok()
        );
        assert!(
            Chart::new(
                ChartKind::Bar,
                "Report".into(),
                "".into(),
                vec![datum("rounded", 9_007_199_254_740_993_u64.into())]
            )
            .is_err()
        );
        assert!(
            Chart::new(
                ChartKind::Pie,
                "Report".into(),
                "".into(),
                vec![datum("a", serde_json::Number::from_f64(1e308).unwrap()); 2]
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_unknown_fields_during_deserialization() {
        assert!(
            serde_json::from_str::<Chart>(
                r#"{"kind":"bar","title":"x","description":"y","data":[],"extra":true}"#
            )
            .is_err()
        );
    }

    #[test]
    fn offline_html_escapes_user_text_and_keeps_data_out_of_script_tags() {
        let chart = Chart::new(
            ChartKind::Bar,
            "<script>title</script>".into(),
            "& details".into(),
            vec![datum("</script><script>alert(1)</script>", 1.into())],
        )
        .unwrap();
        let html = chart.html().unwrap();
        assert!(!html.contains("<script>title</script>"));
        assert!(html.contains("&lt;script&gt;title&lt;/script&gt;"));
        assert!(html.contains("&amp; details"));
        assert!(html.contains(r#"\u003c/script\u003e\u003cscript\u003e"#));
        assert!(html.contains("&lt;/script&gt;&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(html.contains("View chart data"));
        assert!(html.contains(CHART_JS));
        assert!(!html.contains("cdn.jsdelivr.net"));
        assert!(html.contains("prefers-reduced-motion"));
        assert!(html.contains("color-scheme"));
        assert!(html.contains(&format!("Source: {}", chart.source())));
        assert_eq!(chart.filename(), "script-title-script.html");
        let largest = Chart::new(
            ChartKind::Bar,
            "<".repeat(MAX_TITLE_UTF16),
            "&".repeat(MAX_DESCRIPTION_UTF16),
            vec![datum(&"<".repeat(MAX_LABEL_UTF16), 1.into()); MAX_POINTS],
        )
        .unwrap();
        assert!(largest.html().unwrap().len() <= MAX_CHART_EXPORT_BYTES);
    }

    #[test]
    fn filename_is_safe_for_titles_without_ascii_words() {
        assert_eq!(chart(ChartKind::Pie, vec![]).filename(), "report.html");
        let untitled = Chart::new(ChartKind::Bar, "你好".into(), "".into(), vec![]).unwrap();
        assert_eq!(untitled.filename(), "visualization.html");
    }
}
