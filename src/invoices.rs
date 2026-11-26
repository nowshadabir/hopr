use crate::auth::AUTH_API_BASE;
use crate::config::AppConfig;
use crate::platform::downloads_dir;
use crate::webview_manager::AppEvent;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tao::event_loop::EventLoopProxy;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InvoiceBusinessProfile {
    #[serde(default)]
    pub business_name: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub hotline: String,
    #[serde(default)]
    pub email: String,
    #[serde(default = "default_mushok")]
    pub mushok_badge: String,
    #[serde(default)]
    pub logo_data_url: Option<String>,
}

fn default_mushok() -> String {
    "MUSHOK 6.3".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceRoomItem {
    pub description: String,
    #[serde(default)]
    pub check_in: String,
    #[serde(default)]
    pub check_out: String,
    #[serde(default = "default_one")]
    pub nights: u32,
    #[serde(default = "default_one")]
    pub rooms: u32,
    #[serde(default)]
    pub rate: f64,
}

fn default_one() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceExtraItem {
    pub label: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceRecord {
    pub id: String,
    pub invoice_no: String,
    #[serde(default = "default_form_title")]
    pub form_title: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub guest_id: String,
    #[serde(default)]
    pub reg_date: String,
    #[serde(default)]
    pub guest_name: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub guest_address: String,
    #[serde(default)]
    pub guest_phone: String,
    #[serde(default)]
    pub check_in: String,
    #[serde(default)]
    pub check_out: String,
    #[serde(default)]
    pub pax: String,
    #[serde(default)]
    pub duration: String,
    #[serde(default)]
    pub booked_by: String,
    #[serde(default)]
    pub special_req: String,
    #[serde(default)]
    pub items: Vec<InvoiceRoomItem>,
    #[serde(default)]
    pub meal_plan: String,
    #[serde(default)]
    pub extras: Vec<InvoiceExtraItem>,
    #[serde(default)]
    pub discount: f64,
    #[serde(default)]
    pub advance_paid: f64,
    #[serde(default)]
    pub payment_method: String,
    #[serde(default)]
    pub airport_pickup: String,
    #[serde(default)]
    pub airport_drop: String,
    #[serde(default)]
    pub entered_by: String,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default)]
    pub amount_in_words: String,
    #[serde(default)]
    pub synced: bool,
    #[serde(default)]
    pub updated_at_epoch: u64,
}

fn default_form_title() -> String {
    "RESERVATION FORM (GUEST COPY)".to_string()
}

fn default_currency() -> String {
    "BDT".to_string()
}

impl InvoiceRecord {
    pub fn room_charges(&self) -> f64 {
        self.items
            .iter()
            .map(|it| (it.nights.max(1) as f64) * (it.rooms.max(1) as f64) * it.rate)
            .sum()
    }

    pub fn extras_total(&self) -> f64 {
        self.extras.iter().map(|ex| ex.amount).sum()
    }

    pub fn net_payable(&self) -> f64 {
        (self.room_charges() + self.extras_total() - self.discount).max(0.0)
    }

    pub fn refund_due(&self) -> f64 {
        (self.advance_paid - self.net_payable()).max(0.0)
    }

    pub fn balance_due(&self) -> f64 {
        (self.net_payable() - self.advance_paid).max(0.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LocalInvoiceStore {
    #[serde(default)]
    pub profile: InvoiceBusinessProfile,
    #[serde(default)]
    pub invoices: Vec<InvoiceRecord>,
    #[serde(default)]
    pub has_initial_sync: bool,
}

impl LocalInvoiceStore {
    fn store_path(user_id: u64) -> PathBuf {
        AppConfig::app_support_dir().join(format!("invoices_{user_id}.json"))
    }

    pub fn load(user_id: u64) -> Self {
        let path = Self::store_path(user_id);
        if let Ok(raw) = fs::read_to_string(&path)
            && let Ok(store) = serde_json::from_str::<Self>(&raw)
        {
            return store;
        }
        Self::default()
    }

    pub fn save(&self, user_id: u64) {
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            let _ = AppConfig::write_secure_file(&Self::store_path(user_id), &bytes);
        }
    }

    pub fn upsert_invoice(&mut self, mut inv: InvoiceRecord) -> InvoiceRecord {
        if inv.id.trim().is_empty() {
            inv.id = Uuid::new_v4().to_string();
        }
        inv.updated_at_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if let Some(pos) = self.invoices.iter().position(|x| x.id == inv.id) {
            self.invoices[pos] = inv.clone();
        } else {
            self.invoices.insert(0, inv.clone());
        }
        inv
    }

    pub fn remove_invoice(&mut self, id: &str) -> bool {
        let before = self.invoices.len();
        self.invoices.retain(|x| x.id != id);
        self.invoices.len() != before
    }
}

#[derive(Debug, Clone)]
pub enum InvoiceOutcome {
    SyncedList {
        business_name: Option<String>,
        invoices: Vec<InvoiceRecord>,
    },
    SavedOne {
        id: String,
    },
    Error {
        message: String,
    },
}

#[derive(Deserialize)]
struct ApiInvoicesListResp {
    ok: bool,
    business_name: Option<String>,
    #[serde(default)]
    invoices: Vec<InvoiceRecord>,
    message: Option<String>,
}

#[derive(Deserialize)]
struct ApiGenericResp {
    ok: bool,
    message: Option<String>,
}

fn http_agent() -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(15));
    if let Ok(connector) = native_tls::TlsConnector::new() {
        builder = builder.tls_connector(Arc::new(connector));
    }
    builder.build()
}

pub fn spawn_fetch_invoices(proxy: EventLoopProxy<AppEvent>, token: String) {
    thread::spawn(move || {
        let url = format!("{AUTH_API_BASE}/invoices");
        let auth_header = format!("Bearer {token}");
        let outcome = match http_agent()
            .get(&url)
            .set("Authorization", &auth_header)
            .call()
        {
            Ok(resp) => match resp.into_json::<ApiInvoicesListResp>() {
                Ok(data) if data.ok => {
                    let invoices = data
                        .invoices
                        .into_iter()
                        .map(|mut inv| {
                            inv.synced = true;
                            inv
                        })
                        .collect();
                    InvoiceOutcome::SyncedList {
                        business_name: data.business_name,
                        invoices,
                    }
                }
                Ok(data) => InvoiceOutcome::Error {
                    message: data.message.unwrap_or_else(|| "Failed to load invoices.".to_string()),
                },
                Err(e) => InvoiceOutcome::Error {
                    message: format!("Invalid invoice response: {e}"),
                },
            },
            Err(e) => InvoiceOutcome::Error {
                message: format!("Sync failed: {e}"),
            },
        };
        let _ = proxy.send_event(AppEvent::InvoiceResult(outcome));
    });
}

pub fn spawn_save_invoice(
    proxy: EventLoopProxy<AppEvent>,
    token: String,
    business_name: String,
    invoice: InvoiceRecord,
) {
    thread::spawn(move || {
        let url = format!("{AUTH_API_BASE}/invoices");
        let auth_header = format!("Bearer {token}");
        let id = invoice.id.clone();
        let payload = serde_json::json!({
            "business_name": business_name,
            "invoice": invoice,
        });
        let outcome = match http_agent()
            .post(&url)
            .set("Authorization", &auth_header)
            .send_json(payload)
        {
            Ok(resp) => match resp.into_json::<ApiGenericResp>() {
                Ok(data) if data.ok => InvoiceOutcome::SavedOne { id },
                Ok(data) => InvoiceOutcome::Error {
                    message: data.message.unwrap_or_else(|| "Failed to save invoice to cloud.".to_string()),
                },
                Err(e) => InvoiceOutcome::Error {
                    message: format!("Invalid save response: {e}"),
                },
            },
            Err(e) => InvoiceOutcome::Error {
                message: format!("Saved locally (cloud sync pending): {e}"),
            },
        };
        let _ = proxy.send_event(AppEvent::InvoiceResult(outcome));
    });
}

pub fn spawn_delete_invoice(proxy: EventLoopProxy<AppEvent>, token: String, id: String) {
    thread::spawn(move || {
        let url = format!("{AUTH_API_BASE}/invoices/{id}");
        let auth_header = format!("Bearer {token}");
        let _ = http_agent()
            .delete(&url)
            .set("Authorization", &auth_header)
            .call();
        let _ = proxy;
    });
}

fn pdf_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            '৳' => out.push_str("BDT "),
            _ => out.push(' '),
        }
    }
    out
}

fn fmt_money(currency: &str, val: f64) -> String {
    let prefix = if currency.eq_ignore_ascii_case("BDT") || currency == "৳" {
        "BDT "
    } else {
        currency
    };
    format!("{prefix}{:.0}", val.round())
}

pub fn export_invoice_pdf(
    inv: &InvoiceRecord,
    profile: &InvoiceBusinessProfile,
) -> Result<PathBuf, String> {
    export_invoice_pdf_to_dir(inv, profile, &downloads_dir())
}

fn export_invoice_pdf_to_dir(
    inv: &InvoiceRecord,
    profile: &InvoiceBusinessProfile,
    dir: &std::path::Path,
) -> Result<PathBuf, String> {
    let _ = fs::create_dir_all(dir);
    let safe_no: String = inv
        .invoice_no
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let base_name = if safe_no.is_empty() {
        "Invoice".to_string()
    } else {
        format!("Invoice-{safe_no}")
    };
    let mut dest = dir.join(format!("{base_name}.pdf"));
    if dest.exists() {
        for n in 1..1000 {
            let next = dir.join(format!("{base_name} ({n}).pdf"));
            if !next.exists() {
                dest = next;
                break;
            }
        }
    }

    // Build clean A4 (595 x 842 pt) PDF content stream
    let mut ops = String::new();
    let draw_text = |ops: &mut String, x: f32, y: f32, size: f32, bold: bool, text: &str| {
        let font = if bold { "/F2" } else { "/F1" };
        ops.push_str(&format!(
            "BT {font} {size} Tf 1 0 0 1 {x:.1} {y:.1} Tm ({}) Tj ET\n",
            pdf_escape(text)
        ));
    };
    let draw_line = |ops: &mut String, x1: f32, y1: f32, x2: f32, y2: f32| {
        ops.push_str(&format!("0.6 w 0.75 0.75 0.75 RG {x1:.1} {y1:.1} m {x2:.1} {y2:.1} l S\n"));
    };

    let biz = if profile.business_name.trim().is_empty() {
        "Hopr Business"
    } else {
        profile.business_name.trim()
    };
    draw_text(&mut ops, 40.0, 800.0, 16.0, true, biz);
    if !profile.mushok_badge.trim().is_empty() {
        draw_text(&mut ops, 470.0, 802.0, 9.0, true, &profile.mushok_badge);
    }
    draw_text(&mut ops, 40.0, 782.0, 9.0, false, &inv.created_at);
    draw_text(&mut ops, 430.0, 782.0, 10.0, true, &format!("Invoice No: {}", inv.invoice_no));
    draw_line(&mut ops, 40.0, 774.0, 555.0, 774.0);

    draw_text(&mut ops, 180.0, 756.0, 12.0, true, &inv.form_title);
    draw_line(&mut ops, 40.0, 746.0, 555.0, 746.0);

    // Guest & Stay 2-column block
    let left_rows = [
        ("Guest ID", inv.guest_id.as_str()),
        ("Reg. Date", inv.reg_date.as_str()),
        ("Group/Name", inv.guest_name.as_str()),
        ("Source", inv.source.as_str()),
        ("Address", inv.guest_address.as_str()),
        ("Phone", inv.guest_phone.as_str()),
    ];
    let right_rows = [
        ("Check In", inv.check_in.as_str()),
        ("Check Out", inv.check_out.as_str()),
        ("Pax", inv.pax.as_str()),
        ("Duration", inv.duration.as_str()),
        ("Booked by", inv.booked_by.as_str()),
    ];
    let mut y = 730.0;
    for (k, v) in left_rows {
        draw_text(&mut ops, 44.0, y, 9.0, true, k);
        draw_text(&mut ops, 115.0, y, 9.0, false, &format!(": {v}"));
        y -= 14.0;
    }
    let mut ry = 730.0;
    for (k, v) in right_rows {
        draw_text(&mut ops, 320.0, ry, 9.0, true, k);
        draw_text(&mut ops, 385.0, ry, 9.0, false, &format!(": {v}"));
        ry -= 14.0;
    }
    draw_text(&mut ops, 44.0, 642.0, 9.0, true, "Special Req.");
    draw_text(&mut ops, 115.0, 642.0, 9.0, false, &format!(": {}", inv.special_req));
    draw_line(&mut ops, 40.0, 632.0, 555.0, 632.0);

    // Items table header
    draw_text(&mut ops, 44.0, 616.0, 8.5, true, "#");
    draw_text(&mut ops, 64.0, 616.0, 8.5, true, "DESCRIPTION");
    draw_text(&mut ops, 240.0, 616.0, 8.5, true, "CHECK IN");
    draw_text(&mut ops, 315.0, 616.0, 8.5, true, "CHECK OUT");
    draw_text(&mut ops, 390.0, 616.0, 8.5, true, "NIGHT(S)");
    draw_text(&mut ops, 442.0, 616.0, 8.5, true, "ROOM(S)");
    draw_text(&mut ops, 490.0, 616.0, 8.5, true, "RATE");
    draw_text(&mut ops, 528.0, 616.0, 8.5, true, "AMOUNT");
    draw_line(&mut ops, 40.0, 610.0, 555.0, 610.0);

    let mut iy = 594.0;
    for (idx, item) in inv.items.iter().take(12).enumerate() {
        let amt = (item.nights.max(1) as f64) * (item.rooms.max(1) as f64) * item.rate;
        draw_text(&mut ops, 44.0, iy, 9.0, false, &(idx + 1).to_string());
        draw_text(&mut ops, 64.0, iy, 9.0, false, &item.description);
        draw_text(&mut ops, 240.0, iy, 8.5, false, &item.check_in);
        draw_text(&mut ops, 315.0, iy, 8.5, false, &item.check_out);
        draw_text(&mut ops, 400.0, iy, 9.0, false, &item.nights.to_string());
        draw_text(&mut ops, 452.0, iy, 9.0, false, &item.rooms.to_string());
        draw_text(&mut ops, 490.0, iy, 9.0, false, &format!("{:.0}", item.rate));
        draw_text(&mut ops, 525.0, iy, 9.0, true, &format!("{:.0}", amt));
        iy -= 16.0;
    }
    draw_line(&mut ops, 40.0, iy + 6.0, 555.0, iy + 6.0);

    let sy = (iy - 14.0).max(260.0);
    // Left summary: Meal plan & Extras
    let mut ly = sy;
    if !inv.meal_plan.trim().is_empty() {
        draw_text(&mut ops, 44.0, ly, 9.0, true, "MEAL PLAN:");
        ly -= 13.0;
        draw_text(&mut ops, 44.0, ly, 9.0, false, &inv.meal_plan);
        ly -= 18.0;
    }
    if !inv.extras.is_empty() {
        draw_text(&mut ops, 44.0, ly, 9.0, true, "EXTRA SERVICES & ITEMS:");
        ly -= 13.0;
        for ex in inv.extras.iter().take(8) {
            let tag = if ex.tag.is_empty() {
                String::new()
            } else {
                format!(" [{}]", ex.tag)
            };
            draw_text(&mut ops, 44.0, ly, 8.5, false, &format!("{}{tag}", ex.label));
            draw_text(&mut ops, 245.0, ly, 8.5, true, &fmt_money(&inv.currency, ex.amount));
            ly -= 13.0;
        }
    }
    if !inv.amount_in_words.trim().is_empty() {
        ly -= 8.0;
        draw_text(&mut ops, 44.0, ly, 9.0, true, &inv.amount_in_words);
    }

    // Right financial summary
    let mut fy = sy;
    let fin_rows = [
        ("Room Charges", fmt_money(&inv.currency, inv.room_charges()), false),
        ("Add-ons / Services", format!("+{}", fmt_money(&inv.currency, inv.extras_total())), false),
        ("Discount", format!("-{}", fmt_money(&inv.currency, inv.discount)), false),
        ("Net Payable Amount", fmt_money(&inv.currency, inv.net_payable()), true),
        ("Advance/Deposit Paid", fmt_money(&inv.currency, inv.advance_paid), false),
        ("Refund Due to Guest", fmt_money(&inv.currency, inv.refund_due()), true),
        ("Balance Due", fmt_money(&inv.currency, inv.balance_due()), true),
    ];
    for (k, v, bold) in fin_rows {
        draw_text(&mut ops, 330.0, fy, 9.0, bold, k);
        draw_text(&mut ops, 470.0, fy, 9.0, bold, &v);
        fy -= 15.0;
    }
    if !inv.payment_method.trim().is_empty() {
        draw_text(&mut ops, 330.0, fy - 2.0, 8.0, false, &format!("Paid via: {}", inv.payment_method));
    }

    let ay = fy.min(ly) - 24.0;
    draw_line(&mut ops, 40.0, ay + 14.0, 555.0, ay + 14.0);
    draw_text(&mut ops, 44.0, ay, 9.0, false, &format!("Airport Pickup: {}", inv.airport_pickup));
    draw_text(&mut ops, 320.0, ay, 9.0, false, &format!("Airport Drop: {}", inv.airport_drop));

    // Signatures & Footer
    draw_line(&mut ops, 40.0, 110.0, 175.0, 110.0);
    draw_text(&mut ops, 68.0, 96.0, 9.0, true, "Guest Signature");
    draw_line(&mut ops, 230.0, 110.0, 365.0, 110.0);
    let entered = if inv.entered_by.trim().is_empty() {
        "Entered by".to_string()
    } else {
        format!("Entered by: {}", inv.entered_by)
    };
    draw_text(&mut ops, 255.0, 96.0, 9.0, true, &entered);
    draw_line(&mut ops, 420.0, 110.0, 555.0, 110.0);
    draw_text(&mut ops, 458.0, 96.0, 9.0, true, "Date & Stamp");

    draw_text(&mut ops, 40.0, 62.0, 9.0, true, biz);
    if !profile.address.trim().is_empty() {
        draw_text(&mut ops, 40.0, 49.0, 8.0, false, &profile.address);
    }
    let contact = [
        (!profile.hotline.trim().is_empty()).then(|| format!("Hotline: {}", profile.hotline.trim())),
        (!profile.email.trim().is_empty()).then(|| format!("E-mail: {}", profile.email.trim())),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("   ");
    if !contact.is_empty() {
        draw_text(&mut ops, 40.0, 37.0, 8.0, false, &contact);
    }

    // Assemble minimal valid PDF 1.4
    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n");
    let mut offsets = Vec::new();

    let push_obj = |pdf: &mut Vec<u8>, offsets: &mut Vec<usize>, num: usize, body: &[u8]| {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    };

    push_obj(&mut pdf, &mut offsets, 1, b"<< /Type /Catalog /Pages 2 0 R >>");
    push_obj(&mut pdf, &mut offsets, 2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    push_obj(
        &mut pdf,
        &mut offsets,
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 4 0 R /F2 5 0 R >> >> /Contents 6 0 R >>",
    );
    push_obj(&mut pdf, &mut offsets, 4, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    push_obj(&mut pdf, &mut offsets, 5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>");
    let stream_bytes = ops.as_bytes();
    let mut obj6 = format!("<< /Length {} >>\nstream\n", stream_bytes.len()).into_bytes();
    obj6.extend_from_slice(stream_bytes);
    obj6.extend_from_slice(b"\nendstream");
    push_obj(&mut pdf, &mut offsets, 6, &obj6);

    let xref_pos = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for off in offsets {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n").as_bytes(),
    );

    fs::write(&dest, pdf).map_err(|e| e.to_string())?;
    Ok(dest)
}

pub fn whatsapp_send_url(inv: &InvoiceRecord, profile: &InvoiceBusinessProfile) -> String {
    let digits: String = inv.guest_phone.chars().filter(|c| c.is_ascii_digit()).collect();
    let biz = if profile.business_name.trim().is_empty() {
        "Hopr"
    } else {
        profile.business_name.trim()
    };
    let text = format!(
        "*{biz}*\n{title}\nInvoice No: {no}\nGuest: {guest}\nCheck-in: {ci} | Check-out: {co}\nNet Payable: {net}\nAdvance Paid: {adv}\nBalance Due: {bal}",
        title = inv.form_title,
        no = inv.invoice_no,
        guest = inv.guest_name,
        ci = inv.check_in,
        co = inv.check_out,
        net = fmt_money(&inv.currency, inv.net_payable()),
        adv = fmt_money(&inv.currency, inv.advance_paid),
        bal = fmt_money(&inv.currency, inv.balance_due()),
    );
    let encoded: String = text
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "%20".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    if digits.is_empty() {
        format!("https://web.whatsapp.com/send?text={encoded}")
    } else {
        format!("https://web.whatsapp.com/send?phone={digits}&text={encoded}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invoice_math_and_pdf_export_work() {
        let inv = InvoiceRecord {
            id: "inv-1".to_string(),
            invoice_no: "904064".to_string(),
            form_title: default_form_title(),
            created_at: "27-Nov-26 01:18 AM".to_string(),
            guest_id: "GST-0207AE".to_string(),
            reg_date: "28-Nov-26".to_string(),
            guest_name: "Abir".to_string(),
            source: "DIRECT".to_string(),
            guest_address: "-".to_string(),
            guest_phone: "01618129800".to_string(),
            check_in: "28-Nov-26 02:00 PM".to_string(),
            check_out: "29-Nov-26 12:00 PM".to_string(),
            pax: "2".to_string(),
            duration: "1 Nights".to_string(),
            booked_by: "shahadat".to_string(),
            special_req: "-".to_string(),
            items: vec![InvoiceRoomItem {
                description: "Couple AC".to_string(),
                check_in: "28-Nov-26".to_string(),
                check_out: "29-Nov-26".to_string(),
                nights: 1,
                rooms: 1,
                rate: 3500.0,
            }],
            meal_plan: "Breakfast Included (BB Plan)".to_string(),
            extras: vec![InvoiceExtraItem {
                label: "extra bed".to_string(),
                tag: "Room 102".to_string(),
                amount: 600.0,
            }],
            discount: 3500.0,
            advance_paid: 1750.0,
            payment_method: "DIRECT TO HOTEL · CASH".to_string(),
            airport_pickup: "NO".to_string(),
            airport_drop: "NO".to_string(),
            entered_by: "shahadat".to_string(),
            currency: "BDT".to_string(),
            amount_in_words: "Six Hundred Taka Only".to_string(),
            synced: false,
            updated_at_epoch: 0,
        };
        assert_eq!(inv.room_charges(), 3500.0);
        assert_eq!(inv.extras_total(), 600.0);
        assert_eq!(inv.net_payable(), 600.0);
        assert_eq!(inv.refund_due(), 1150.0);
        assert_eq!(inv.balance_due(), 0.0);

        let tmp = std::env::temp_dir().join(format!("hopr-pdf-test-{}", Uuid::new_v4()));
        let profile = InvoiceBusinessProfile {
            business_name: "LODGE KOLATOLI".to_string(),
            ..Default::default()
        };
        let pdf_path = export_invoice_pdf_to_dir(&inv, &profile, &tmp).unwrap();
        let bytes = fs::read(&pdf_path).unwrap();
        assert!(bytes.starts_with(b"%PDF-1.4"));
        let _ = fs::remove_dir_all(&tmp);
    }
}
