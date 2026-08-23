use printpdf::*;
use rust_decimal::Decimal;

const PAGE_WIDTH_MM: f32 = 210.0;
const PAGE_HEIGHT_MM: f32 = 297.0;
const MARGIN_MM: f32 = 20.0;
const BOTTOM_MARGIN_MM: f32 = 20.0;

const COL_HEADER_RIGHT: f32 = 135.0;
const COL_GST_RATE: f32 = 112.0;
const COL_SAC: f32 = 142.0;
const COL_AMOUNT: f32 = 165.0;

/// Minimal line-based PDF builder — invoices/receipts/NOCs/statements are all just
/// title + a handful of label/value lines + an optional table, nothing needs real
/// layout. Uses only the PDF standard-14 Helvetica font, so no font file ships with
/// the repo. Starts a new page automatically when content runs past the bottom margin.
struct SimpleDoc {
    doc: PdfDocument,
    pages: Vec<PdfPage>,
    ops: Vec<Op>,
    y: f32,
    /// Where the PDF text cursor actually is right now, in Mm from the page origin.
    /// `Op::SetTextCursor` compiles to the PDF `Td` operator, which moves the cursor by
    /// an OFFSET from its current position, not to an absolute point — every call here
    /// must pass the delta from this, not the raw target coordinates, or text after the
    /// first line ends up positioned off the page. Resets to (0, 0) on every new `BT`
    /// (initial page and every page break), since that's where a fresh text section's
    /// cursor starts.
    cursor_mm: (f32, f32),
}

impl SimpleDoc {
    fn new(title: &str) -> Self {
        let mut s = Self {
            doc: PdfDocument::new(title),
            pages: Vec::new(),
            ops: Vec::new(),
            y: PAGE_HEIGHT_MM - MARGIN_MM,
            cursor_mm: (0.0, 0.0),
        };
        s.ops.push(Op::StartTextSection);
        s
    }

    fn ensure_space(&mut self, needed_mm: f32) {
        if self.y - needed_mm < BOTTOM_MARGIN_MM {
            self.ops.push(Op::EndTextSection);
            let page = PdfPage::new(
                Mm(PAGE_WIDTH_MM),
                Mm(PAGE_HEIGHT_MM),
                std::mem::take(&mut self.ops),
            );
            self.pages.push(page);
            self.y = PAGE_HEIGHT_MM - MARGIN_MM;
            self.cursor_mm = (0.0, 0.0);
            self.ops.push(Op::StartTextSection);
        }
    }

    fn move_cursor_to(&mut self, x: f32, y: f32) {
        let (last_x, last_y) = self.cursor_mm;
        self.ops.push(Op::SetTextCursor {
            pos: Point::new(Mm(x - last_x), Mm(y - last_y)),
        });
        self.cursor_mm = (x, y);
    }

    /// Places `text` at absolute `(x, self.y)` without advancing `y` — the building
    /// block for both single-column lines and multi-column table rows, where several
    /// `text_at` calls share one row before a single `advance_line`.
    fn text_at(&mut self, x: f32, text: &str, size: f32, bold: bool) {
        self.ops.push(Op::SetFont {
            font: PdfFontHandle::Builtin(if bold {
                BuiltinFont::HelveticaBold
            } else {
                BuiltinFont::Helvetica
            }),
            size: Pt(size),
        });
        self.ops.push(Op::SetLineHeight { lh: Pt(size) });
        self.move_cursor_to(x, self.y);
        self.ops.push(Op::ShowText {
            items: vec![TextItem::Text(text.to_string())],
        });
    }

    fn advance_line(&mut self, size: f32) {
        self.y -= size / 2.0 + 2.0;
    }

    fn line(&mut self, text: &str, size: f32) {
        self.ensure_space(size / 2.0);
        self.text_at(MARGIN_MM, text, size, false);
        self.advance_line(size);
    }

    fn bold_line(&mut self, text: &str, size: f32) {
        self.ensure_space(size / 2.0);
        self.text_at(MARGIN_MM, text, size, true);
        self.advance_line(size);
    }

    /// Left-aligned label at the margin, right-aligned label starting at a fixed
    /// column — the "Customer ID ... Date" / "GST No: ... TAX INVOICE" pairing the
    /// reference bill uses throughout its header. Not true right-justification (no
    /// font-metrics table available), just a second fixed column, which reads the same
    /// for short labels like these.
    fn two_col_line(&mut self, left: &str, right: &str, size: f32, right_x: f32, bold: bool) {
        self.ensure_space(size / 2.0);
        if !left.is_empty() {
            self.text_at(MARGIN_MM, left, size, bold);
        }
        if !right.is_empty() {
            self.text_at(right_x, right, size, bold);
        }
        self.advance_line(size);
    }

    /// Helvetica isn't monospace and this has no real glyph-width table to measure
    /// against, so every width-dependent layout decision (wrapping, centering) uses
    /// this same estimate — Helvetica's known average advance width (~0.5em).
    fn estimate_width_mm(text: &str, size: f32) -> f32 {
        let avg_char_width_mm = size * 0.5 * 0.352778;
        text.chars().count() as f32 * avg_char_width_mm
    }

    /// Wraps `text` into rows that fit within the page margins, without printing them —
    /// the shared building block `wrapped_line`/`centered_wrapped_line` split on.
    fn wrap_to_rows(text: &str, size: f32) -> Vec<String> {
        let avg_char_width_mm = size * 0.5 * 0.352778;
        let usable_width_mm = PAGE_WIDTH_MM - 2.0 * MARGIN_MM;
        let max_chars = ((usable_width_mm / avg_char_width_mm).floor() as usize).max(20);

        let mut rows = Vec::new();
        let mut current = String::new();
        for word in text.split(' ') {
            let word_chars = word.chars().count();
            let candidate_len = if current.is_empty() {
                word_chars
            } else {
                current.chars().count() + 1 + word_chars
            };
            if candidate_len > max_chars && !current.is_empty() {
                rows.push(std::mem::take(&mut current));
                current = word.to_string();
            } else {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            rows.push(current);
        }
        rows
    }

    /// Word-wraps `text` to fit within the page margins before printing, one `line()`
    /// call per wrapped row — keeps long dynamic text (bank details, notes) from
    /// silently running off the page edge, which `line()` alone does nothing to
    /// prevent.
    fn wrapped_line(&mut self, text: &str, size: f32) {
        for row in Self::wrap_to_rows(text, size) {
            self.line(&row, size);
        }
    }

    /// Horizontally centers `text` on the page, estimating its width the same way
    /// `wrapped_line` estimates wrap points. Falls back to the left margin if the
    /// estimated width would push it past the margin (long single "word" with nowhere
    /// to wrap).
    fn centered_line(&mut self, text: &str, size: f32, bold: bool) {
        self.ensure_space(size / 2.0);
        let width = Self::estimate_width_mm(text, size);
        let x = ((PAGE_WIDTH_MM - width) / 2.0).max(MARGIN_MM);
        self.text_at(x, text, size, bold);
        self.advance_line(size);
    }

    /// Centered version of `wrapped_line` — each wrapped row is independently centered,
    /// the way a letterhead's address block reads.
    fn centered_wrapped_line(&mut self, text: &str, size: f32) {
        for row in Self::wrap_to_rows(text, size) {
            self.centered_line(&row, size, false);
        }
    }

    fn space(&mut self, mm: f32) {
        self.y -= mm;
    }

    /// A bolder divider than `rule()` — two hairlines close together, used to set the
    /// letterhead block off from the invoice body.
    fn double_rule(&mut self) {
        self.rule();
        self.space(0.8);
        self.rule();
    }

    fn rule(&mut self) {
        self.ensure_space(4.0);
        self.ops.push(Op::SetOutlineThickness { pt: Pt(0.5) });
        self.ops.push(Op::SetOutlineColor {
            col: Color::Rgb(Rgb {
                r: 0.6,
                g: 0.6,
                b: 0.6,
                icc_profile: None,
            }),
        });
        self.ops.push(Op::DrawLine {
            line: Line {
                points: vec![
                    LinePoint {
                        p: Point::new(Mm(MARGIN_MM), Mm(self.y)),
                        bezier: false,
                    },
                    LinePoint {
                        p: Point::new(Mm(PAGE_WIDTH_MM - MARGIN_MM), Mm(self.y)),
                        bezier: false,
                    },
                ],
                is_closed: false,
            },
        });
        self.y -= 4.0;
    }

    fn finish(mut self) -> Vec<u8> {
        self.ops.push(Op::EndTextSection);
        self.pages.push(PdfPage::new(
            Mm(PAGE_WIDTH_MM),
            Mm(PAGE_HEIGHT_MM),
            self.ops,
        ));
        let mut warnings = Vec::new();
        self.doc
            .with_pages(self.pages)
            .save(&PdfSaveOptions::default(), &mut warnings)
    }
}

fn fmt_amount(amount: Decimal) -> String {
    format!("₹{:.2}", amount)
}

fn fmt_plain(amount: Decimal) -> String {
    format!("{:.2}", amount)
}

fn flat_code(wing: &str, flat_number: &str) -> String {
    format!("{}{}", wing, flat_number)
}

/// One line item as shown on the invoice's particulars table — carries the rate/SAC
/// code actually applied at posting time (frozen onto `invoice_line_items`), not the
/// chart-of-accounts row it posted to.
pub struct InvoiceLineItemView<'a> {
    pub description: &'a str,
    pub amount: Decimal,
    pub gst_rate_pct: Decimal,
    pub hsn_sac_code: Option<&'a str>,
    pub gst_amount: Decimal,
}

/// `K0302 - MAINTENANCE BILL FOR JULY 2026 TO SEPTEMBER 2026` — a combined tax-invoice
/// layout matching the bill format residents already receive: society letterhead, a
/// Particulars/GST-Rate/SAC-Code/Amount table split into non-taxable and taxable
/// sub-totals, CGST+SGST (the standard intra-state 50/50 split of the combined GST
/// rate), and a notes/instructions footer built from the actually-configured interest
/// rate and payment instructions — nothing here is hardcoded copy.
#[allow(clippy::too_many_arguments)]
pub fn render_invoice_pdf(
    invoice: &crate::db::Invoice,
    line_items: &[InvoiceLineItemView],
    wing: &str,
    flat_number: &str,
    society: &Option<crate::db::SocietyProfile>,
    society_gstin: Option<&str>,
    instructions: &Option<crate::db::PaymentInstructions>,
    interest_annual_rate_pct: Option<Decimal>,
    is_sample: bool,
) -> Vec<u8> {
    let period_label = format!(
        "FOR {} TO {}",
        invoice
            .billing_period_start
            .format("%B %Y")
            .to_string()
            .to_uppercase(),
        invoice
            .billing_period_end
            .format("%B %Y")
            .to_string()
            .to_uppercase(),
    );
    let doc_title = format!(
        "{} - MAINTENANCE BILL {}",
        flat_code(wing, flat_number),
        period_label
    );
    let mut doc = SimpleDoc::new(&doc_title);

    match society {
        Some(profile) => {
            doc.centered_line(&profile.name.to_uppercase(), 18.0, true);
            if let Some(reg) = &profile.registration_number {
                doc.centered_line(reg, 8.5, false);
            }
            let mut addr_line = profile.address.clone();
            if let Some(state) = &profile.state {
                addr_line.push_str(&format!(", {}", state));
            }
            if let Some(phone) = &profile.phone {
                addr_line.push_str(&format!("  Tel: {}", phone));
            }
            doc.centered_wrapped_line(&addr_line, 8.5);
        }
        None => doc.centered_line(&flat_code(wing, flat_number), 18.0, true),
    }
    if let Some(gstin) = society_gstin {
        doc.centered_line(&format!("GST No: {}", gstin), 9.0, false);
    }
    doc.space(2.0);
    doc.centered_line("TAX INVOICE", 15.0, true);
    if is_sample {
        doc.centered_line(
            "SAMPLE PREVIEW - figures shown are illustrative, not a real bill.",
            8.5,
            false,
        );
    }
    doc.double_rule();

    doc.two_col_line(
        &format!("Customer ID: {}", flat_code(wing, flat_number)),
        &format!("Date: {}", invoice.created_at.format("%d-%m-%Y")),
        10.0,
        COL_HEADER_RIGHT,
        false,
    );
    doc.two_col_line(
        "",
        &format!("Bill No: {}", invoice.invoice_number),
        10.0,
        COL_HEADER_RIGHT,
        false,
    );
    doc.two_col_line(
        &format!("Period: {}", period_label),
        &format!("Due Date: {}", invoice.due_date.format("%d-%m-%Y")),
        10.0,
        COL_HEADER_RIGHT,
        false,
    );
    doc.rule();

    doc.text_at(MARGIN_MM, "Particulars", 9.0, true);
    doc.text_at(COL_GST_RATE, "GST Rate", 9.0, true);
    doc.text_at(COL_SAC, "SAC Code", 9.0, true);
    doc.text_at(COL_AMOUNT, "Amount", 9.0, true);
    doc.advance_line(9.0);
    doc.rule();

    let non_taxable: Vec<&InvoiceLineItemView> = line_items
        .iter()
        .filter(|li| li.gst_amount <= Decimal::ZERO)
        .collect();
    let taxable: Vec<&InvoiceLineItemView> = line_items
        .iter()
        .filter(|li| li.gst_amount > Decimal::ZERO)
        .collect();

    let mut non_taxable_total = Decimal::ZERO;
    for li in &non_taxable {
        doc.ensure_space(4.5);
        doc.text_at(MARGIN_MM, li.description, 9.0, false);
        doc.text_at(COL_AMOUNT, &fmt_plain(li.amount), 9.0, false);
        doc.advance_line(9.0);
        non_taxable_total += li.amount;
    }
    if !non_taxable.is_empty() {
        doc.ensure_space(4.5);
        doc.text_at(MARGIN_MM, "Sub Total (Non-Taxable)", 9.0, true);
        doc.text_at(COL_AMOUNT, &fmt_plain(non_taxable_total), 9.0, true);
        doc.advance_line(9.0);
    }

    let mut taxable_total = Decimal::ZERO;
    let mut last_gst_rate: Option<Decimal> = None;
    for li in &taxable {
        doc.ensure_space(4.5);
        doc.text_at(MARGIN_MM, li.description, 9.0, false);
        doc.text_at(
            COL_GST_RATE,
            &format!("{:.2}%", li.gst_rate_pct),
            9.0,
            false,
        );
        doc.text_at(COL_SAC, li.hsn_sac_code.unwrap_or("-"), 9.0, false);
        doc.text_at(COL_AMOUNT, &fmt_plain(li.amount), 9.0, false);
        doc.advance_line(9.0);
        taxable_total += li.amount;
        last_gst_rate = Some(li.gst_rate_pct);
    }
    if !taxable.is_empty() {
        doc.ensure_space(4.5);
        doc.text_at(MARGIN_MM, "Sub Total (Taxable Services)", 9.0, true);
        doc.text_at(COL_AMOUNT, &fmt_plain(taxable_total), 9.0, true);
        doc.advance_line(9.0);
    }
    doc.rule();

    if invoice.gst_amount > Decimal::ZERO {
        let half_gst = (invoice.gst_amount / Decimal::from(2)).round_dp(2);
        let half_rate_label = last_gst_rate
            .map(|r| format!("{:.2}%", (r / Decimal::from(2)).round_dp(2)))
            .unwrap_or_default();
        doc.two_col_line(
            &format!("SGST @{}", half_rate_label),
            &fmt_plain(half_gst),
            10.0,
            COL_AMOUNT,
            false,
        );
        doc.two_col_line(
            &format!("CGST @{}", half_rate_label),
            &fmt_plain(invoice.gst_amount - half_gst),
            10.0,
            COL_AMOUNT,
            false,
        );
    }
    doc.two_col_line(
        "Total Current Charges",
        &fmt_plain(invoice.total_amount),
        11.0,
        COL_AMOUNT,
        true,
    );
    if invoice.amount_paid > Decimal::ZERO {
        doc.two_col_line(
            "Already Paid",
            &fmt_plain(invoice.amount_paid),
            10.0,
            COL_AMOUNT,
            false,
        );
        doc.two_col_line(
            "Balance Payable",
            &fmt_plain(invoice.total_amount - invoice.amount_paid),
            11.0,
            COL_AMOUNT,
            true,
        );
    }
    doc.rule();

    doc.bold_line("NOTES / INSTRUCTIONS:", 9.0);
    let mut note_num = 1;
    if let Some(rate) = interest_annual_rate_pct {
        doc.wrapped_line(
            &format!(
                "{}. Please pay by the due date. Interest will be charged @ {:.2}% p.a. from the due date if not paid on or before due date.",
                note_num, rate
            ),
            8.5,
        );
        note_num += 1;
    }
    if let Some(pi) = instructions {
        doc.wrapped_line(
            &format!(
                "{}. To pay by NEFT/RTGS: Beneficiary {}, Bank {}, Branch {}, IFSC {}, A/c No {}",
                note_num,
                pi.account_holder_name,
                pi.bank_name,
                pi.branch.as_deref().unwrap_or("-"),
                pi.ifsc_code,
                pi.account_number
            ),
            8.5,
        );
        note_num += 1;
        if let Some(email) = &pi.contact_email {
            doc.line(
                &format!("{}. For queries, contact {}", note_num, email),
                8.5,
            );
            note_num += 1;
        }
    }
    doc.line(
        &format!("{}. Please treat this bill as a demand notice.", note_num),
        8.5,
    );
    doc.space(4.0);
    doc.rule();

    if let Some(profile) = society {
        doc.line(&format!("For {}", profile.name.to_uppercase()), 9.0);
    }
    doc.line(
        "This is a computer generated bill, no signature is required.",
        8.0,
    );
    doc.line("E. & O.E.", 8.0);

    doc.finish()
}

pub fn render_receipt_pdf(
    receipt_number: &str,
    payment: &crate::db::Payment,
    invoice_number: &str,
    wing: &str,
    flat_number: &str,
) -> Vec<u8> {
    let mut doc = SimpleDoc::new(&format!("Receipt {}", receipt_number));
    doc.bold_line("PAYMENT RECEIPT", 16.0);
    doc.space(4.0);
    doc.line(&format!("Receipt Number: {}", receipt_number), 11.0);
    doc.line(
        &format!("Date: {}", payment.confirmed_at.format("%d %b %Y")),
        11.0,
    );
    doc.line(&format!("Unit: {}", flat_code(wing, flat_number)), 11.0);
    doc.line(&format!("Against Invoice: {}", invoice_number), 11.0);
    doc.rule();
    doc.bold_line(
        &format!("Amount Received: {}", fmt_amount(payment.amount)),
        13.0,
    );
    doc.line(&format!("Payment Method: {}", payment.payment_method), 10.0);
    if let Some(reference) = &payment.reference_number {
        doc.line(&format!("Reference: {}", reference), 10.0);
    }
    doc.finish()
}

/// A sample invoice using the society's actually-configured GST rule and payment
/// instructions, but fictitious billing figures — so an admin can check what a real
/// invoice will look like before generating one, without a real flat or invoice to
/// point at.
/// Always shows the full non-taxable/taxable table layout with representative figures,
/// regardless of what's actually configured yet — a preview that degenerates to one
/// bare line whenever GST isn't set up wouldn't show anyone what a real bill looks
/// like. Uses the real GST rule's rate/SAC/GSTIN when one exists; falls back to the
/// standard 18% society-maintenance rate and SAC 999598 when none is configured yet, so
/// the table structure is always demonstrated. Clearly labelled as a sample so the
/// fallback figures are never mistaken for real configuration.
pub fn render_sample_invoice_pdf(
    gst_rule: &Option<crate::db::GstRule>,
    society: &Option<crate::db::SocietyProfile>,
    instructions: &Option<crate::db::PaymentInstructions>,
    interest_annual_rate_pct: Option<Decimal>,
) -> Vec<u8> {
    let today = chrono::Utc::now().date_naive();
    let non_taxable_total = Decimal::from(1400);
    let taxable_base = Decimal::from(4600);

    let (gst_rate_pct, hsn_sac_code, society_gstin): (Decimal, &str, Option<&str>) = match gst_rule
    {
        Some(rule) => (
            rule.gst_rate_pct,
            rule.hsn_sac_code.as_str(),
            rule.society_gstin.as_deref(),
        ),
        None => (Decimal::from(18), "999598", None),
    };
    let gst_amount = (taxable_base * gst_rate_pct / Decimal::from(100)).round_dp(2);
    let total = non_taxable_total + taxable_base + gst_amount;

    let invoice = crate::db::Invoice {
        invoice_id: crate::InvoiceId(uuid::Uuid::nil()),
        invoice_number: "SAMPLE-0001".to_string(),
        flat_id: 0,
        fiscal_year_id: crate::FiscalYearId(0),
        billing_period_start: today,
        billing_period_end: today,
        due_date: today,
        subtotal: non_taxable_total + taxable_base,
        gst_amount,
        total_amount: total,
        amount_paid: Decimal::ZERO,
        status: "UNPAID".to_string(),
        gstin_applicable: gst_amount > Decimal::ZERO,
        created_by: 0,
        created_at: chrono::Utc::now(),
    };
    let line_items = vec![
        InvoiceLineItemView {
            description: "Water Charges",
            amount: Decimal::from(800),
            gst_rate_pct: Decimal::ZERO,
            hsn_sac_code: None,
            gst_amount: Decimal::ZERO,
        },
        InvoiceLineItemView {
            description: "Common Area Electricity",
            amount: Decimal::from(600),
            gst_rate_pct: Decimal::ZERO,
            hsn_sac_code: None,
            gst_amount: Decimal::ZERO,
        },
        InvoiceLineItemView {
            description: "Maintenance Charges",
            amount: taxable_base,
            gst_rate_pct,
            hsn_sac_code: Some(hsn_sac_code),
            gst_amount,
        },
    ];
    render_invoice_pdf(
        &invoice,
        &line_items,
        "A",
        "101",
        society,
        society_gstin,
        instructions,
        interest_annual_rate_pct,
        true,
    )
}

/// A sample receipt with fictitious payment figures — same purpose as
/// `render_sample_invoice_pdf`, for the receipt layout.
pub fn render_sample_receipt_pdf() -> Vec<u8> {
    let payment = crate::db::Payment {
        payment_id: crate::PaymentId(uuid::Uuid::nil()),
        invoice_id: crate::InvoiceId(uuid::Uuid::nil()),
        amount: Decimal::from(5000),
        payment_method: "upi".to_string(),
        reference_number: Some("SAMPLE-REF-001".to_string()),
        recorded_by: 0,
        confirmed_at: chrono::Utc::now(),
    };
    render_receipt_pdf("SAMPLE-RCT-0001", &payment, "SAMPLE-0001", "A", "101")
}

/// One row of a member's running-balance statement, already computed by the caller
/// (see accounting::reports / the ledger-backed statement query).
pub struct StatementRow {
    pub date: chrono::NaiveDate,
    pub description: String,
    pub debit: Decimal,
    pub credit: Decimal,
    pub running_balance: Decimal,
}

pub fn render_statement_pdf(
    wing: &str,
    flat_number: &str,
    period_label: &str,
    rows: &[StatementRow],
) -> Vec<u8> {
    let mut doc = SimpleDoc::new(&format!(
        "Statement of Account - {}",
        flat_code(wing, flat_number)
    ));
    doc.bold_line(
        &format!("Statement of Account: {}", flat_code(wing, flat_number)),
        14.0,
    );
    doc.line(period_label, 10.0);
    doc.rule();
    doc.bold_line(
        "Date       Description                          Debit      Credit     Balance",
        9.0,
    );
    for row in rows {
        doc.line(
            &format!(
                "{}  {:<34}  {:>9}  {:>9}  {:>9}",
                row.date.format("%d-%b-%y"),
                truncate(&row.description, 34),
                fmt_amount(row.debit),
                fmt_amount(row.credit),
                fmt_amount(row.running_balance),
            ),
            9.0,
        );
    }
    doc.rule();
    if let Some(last) = rows.last() {
        doc.bold_line(
            &format!("Closing Balance: {}", fmt_amount(last.running_balance)),
            12.0,
        );
    }
    doc.finish()
}

pub fn render_noc_pdf(
    wing: &str,
    flat_number: &str,
    owner_name: Option<&str>,
    as_of: chrono::NaiveDate,
) -> Vec<u8> {
    let mut doc = SimpleDoc::new(&format!("NOC - {}", flat_code(wing, flat_number)));
    doc.bold_line("NO OBJECTION / DUES CLEARANCE CERTIFICATE", 15.0);
    doc.space(6.0);
    doc.line(
        &format!(
            "This is to certify that unit {} ",
            flat_code(wing, flat_number)
        ),
        11.0,
    );
    if let Some(name) = owner_name {
        doc.line(&format!("(Owner: {})", name), 11.0);
    }
    doc.line(
        "has NO outstanding maintenance dues as of the date below.",
        11.0,
    );
    doc.space(4.0);
    doc.line(
        &format!("Date of Issue: {}", as_of.format("%d %b %Y")),
        10.0,
    );
    doc.space(10.0);
    doc.line(
        "This certificate is issued on the basis of accounting records",
        9.0,
    );
    doc.line(
        "and does not waive any dues that may arise after this date.",
        9.0,
    );
    doc.finish()
}

pub struct AuditPdfEntry {
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub event_label: String,
    pub actor_email: Option<String>,
    pub detail: Option<String>,
}

pub struct FieldChangePdfEntry {
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub field_name: String,
    pub value: String,
    pub actor_email: Option<String>,
}

const AUDIT_COL_EVENT: f32 = 68.0;
const AUDIT_COL_ACTOR: f32 = 108.0;

pub fn render_process_audit_pdf(
    process_name: &str,
    flat_label: Option<&str>,
    process_id: &str,
    entries: &[AuditPdfEntry],
    field_changes: &[FieldChangePdfEntry],
) -> Vec<u8> {
    let mut doc = SimpleDoc::new(&format!("Audit Trail - {}", process_name));
    doc.centered_line("AUDIT TRAIL", 16.0, true);
    doc.space(2.0);
    doc.centered_line(process_name, 12.0, false);
    doc.double_rule();

    doc.two_col_line(
        &format!("Process ID: {}", truncate(process_id, 36)),
        &format!(
            "Generated: {}",
            chrono::Utc::now().format("%d %b %Y, %I:%M %p")
        ),
        9.0,
        COL_HEADER_RIGHT,
        false,
    );
    if let Some(flat) = flat_label {
        doc.two_col_line(&format!("Flat: {}", flat), "", 9.0, COL_HEADER_RIGHT, false);
    }
    doc.rule();

    doc.text_at(MARGIN_MM, "Date / Time", 9.0, true);
    doc.text_at(AUDIT_COL_EVENT, "Event", 9.0, true);
    doc.text_at(AUDIT_COL_ACTOR, "Actor", 9.0, true);
    doc.advance_line(9.0);
    doc.rule();

    if entries.is_empty() {
        doc.line("No audit events recorded.", 9.5);
    }
    for e in entries {
        doc.ensure_space(5.0);
        doc.text_at(
            MARGIN_MM,
            &e.created_at.format("%d %b %Y, %I:%M %p").to_string(),
            8.5,
            false,
        );
        doc.text_at(AUDIT_COL_EVENT, &e.event_label, 8.5, false);
        doc.text_at(
            AUDIT_COL_ACTOR,
            e.actor_email.as_deref().unwrap_or("System"),
            8.5,
            false,
        );
        doc.advance_line(8.5);
        if let Some(detail) = &e.detail {
            doc.ensure_space(4.0);
            doc.text_at(
                AUDIT_COL_EVENT,
                &format!("\u{201c}{}\u{201d}", truncate(detail, 85)),
                8.0,
                false,
            );
            doc.advance_line(8.0);
        }
        doc.space(1.5);
    }
    doc.space(6.0);
    doc.rule();
    doc.line(
        "This is an append-only record - no entry shown here can be edited or removed.",
        8.0,
    );

    doc.space(10.0);
    doc.double_rule();
    doc.centered_line("FIELD CHANGES", 13.0, true);
    doc.space(2.0);
    doc.rule();

    doc.text_at(MARGIN_MM, "Date / Time", 9.0, true);
    doc.text_at(AUDIT_COL_EVENT, "Field", 9.0, true);
    doc.text_at(AUDIT_COL_ACTOR, "Actor", 9.0, true);
    doc.advance_line(9.0);
    doc.rule();

    if field_changes.is_empty() {
        doc.line("No field changes recorded.", 9.5);
    }
    for f in field_changes {
        doc.ensure_space(5.0);
        doc.text_at(
            MARGIN_MM,
            &f.created_at.format("%d %b %Y, %I:%M %p").to_string(),
            8.5,
            false,
        );
        doc.text_at(AUDIT_COL_EVENT, &f.field_name, 8.5, false);
        doc.text_at(
            AUDIT_COL_ACTOR,
            f.actor_email.as_deref().unwrap_or("System"),
            8.5,
            false,
        );
        doc.advance_line(8.5);
        doc.ensure_space(4.0);
        doc.text_at(AUDIT_COL_EVENT, &truncate(&f.value, 85), 8.0, false);
        doc.advance_line(8.0);
        doc.space(1.5);
    }

    doc.finish()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let keep = max.saturating_sub(1);
        format!("{}…", s.chars().take(keep).collect::<String>())
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    /// `Op::SetTextCursor` compiles to the PDF `Td` operator, which is a MOVE-BY, not a
    /// MOVE-TO — every emitted cursor op must be the delta from the previous one.
    /// Simulating that accumulation here (exactly what a real PDF renderer does) and
    /// checking it lands on the intended on-page position is what actually catches a
    /// regression back to absolute coordinates — a text-content-only check would not,
    /// since printpdf's own `extract_text` ignores position entirely.
    #[test]
    fn text_cursor_deltas_accumulate_to_the_intended_on_page_position() {
        let mut doc = SimpleDoc::new("Test");
        doc.bold_line("Title", 14.0);
        doc.line("Line A", 10.0);
        doc.line("Line B", 10.0);
        doc.rule();
        doc.bold_line("Total", 12.0);

        let mut cursor = (0.0f32, 0.0f32);
        let mut visited = Vec::new();
        for op in &doc.ops {
            if let Op::SetTextCursor { pos } = op {
                cursor = (cursor.0 + pos.x.0, cursor.1 + pos.y.0);
                visited.push(cursor);
            }
        }

        assert_eq!(
            visited.len(),
            4,
            "one cursor move per line()/bold_line() call"
        );
        for (x, y) in &visited {
            assert!(
                (0.0..=PAGE_WIDTH_MM * 2.83465).contains(x),
                "x cursor position {} pt landed off the page width",
                x
            );
            assert!(
                (0.0..=PAGE_HEIGHT_MM * 2.83465).contains(y),
                "y cursor position {} pt landed off the page height — this is the exact \
                 bug that made every line after the first invisible",
                y
            );
        }
        // Each successive line must sit strictly below (smaller y than) the one before —
        // proves the cursor is actually descending the page, not stuck or diverging.
        for pair in visited.windows(2) {
            assert!(
                pair[1].1 < pair[0].1,
                "line at {:?} did not sit below the previous line at {:?}",
                pair[1],
                pair[0]
            );
        }
    }

    fn shown_text(ops: &[Op]) -> Vec<String> {
        ops.iter()
            .filter_map(|op| match op {
                Op::ShowText { items } => Some(
                    items
                        .iter()
                        .filter_map(|item| match item {
                            TextItem::Text(t) => Some(t.clone()),
                            _ => None,
                        })
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect()
    }

    /// A long dynamic string (bank details, an address) must be split across several
    /// `line()` calls that each fit the page, and — the actual failure mode caught in
    /// this session — every word from the original text must still be present
    /// somewhere; `line()` alone silently ran text off the page edge with no wrap.
    #[test]
    fn wrapped_line_splits_long_text_without_losing_any_of_it() {
        let mut doc = SimpleDoc::new("Test");
        let long_text = "To pay by NEFT/RTGS: Beneficiary Raheja Vistas CHSL, Bank HDFC Bank Ltd, \
             Branch Kanjurmarg Branch, Mumbai, IFSC HDFC0004989, A/c No RVISTAK0302";
        doc.wrapped_line(long_text, 8.5);

        let rows = shown_text(&doc.ops);
        assert!(
            rows.len() > 1,
            "text this long must wrap across more than one line, got {:?}",
            rows
        );

        let usable_width_mm = PAGE_WIDTH_MM - 2.0 * MARGIN_MM;
        let avg_char_width_mm = 8.5 * 0.5 * 0.352778;
        let max_chars = (usable_width_mm / avg_char_width_mm).floor() as usize;
        for row in &rows {
            assert!(
                row.chars().count() <= max_chars,
                "wrapped row {:?} ({} chars) exceeds the estimated {} chars that fit the page width",
                row,
                row.chars().count(),
                max_chars
            );
        }

        let rejoined: String = rows.join(" ");
        for word in long_text.split_whitespace() {
            assert!(
                rejoined.contains(word),
                "word {:?} from the original text is missing from the wrapped output: {:?}",
                word,
                rows
            );
        }
    }

    #[test]
    fn wrapped_line_leaves_short_text_on_one_line() {
        let mut doc = SimpleDoc::new("Test");
        doc.wrapped_line("Short note.", 8.5);
        let rows = shown_text(&doc.ops);
        assert_eq!(rows, vec!["Short note.".to_string()]);
    }

    fn extract_all_text(pdf_bytes: &[u8]) -> String {
        let mut warnings = Vec::new();
        let parsed = PdfDocument::parse(pdf_bytes, &PdfParseOptions::default(), &mut warnings)
            .expect("generated PDF must parse back cleanly");
        parsed
            .extract_text()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn sample_invoice() -> crate::db::Invoice {
        crate::db::Invoice {
            invoice_id: crate::InvoiceId(uuid::Uuid::new_v4()),
            invoice_number: "INV-000001".to_string(),
            flat_id: 1,
            fiscal_year_id: crate::FiscalYearId(1),
            billing_period_start: chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
            billing_period_end: chrono::NaiveDate::from_ymd_opt(2026, 8, 31).unwrap(),
            due_date: chrono::NaiveDate::from_ymd_opt(2026, 8, 10).unwrap(),
            subtotal: dec!(5000),
            gst_amount: dec!(0),
            total_amount: dec!(5000),
            amount_paid: dec!(0),
            status: "UNPAID".to_string(),
            gstin_applicable: false,
            created_by: 1,
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn invoice_pdf_contains_every_field_it_claims_to_show() {
        let mut invoice = sample_invoice();
        invoice.gst_amount = dec!(900);
        invoice.total_amount = dec!(5900);
        invoice.gstin_applicable = true;
        let line_items = vec![
            InvoiceLineItemView {
                description: "Water Charges",
                amount: dec!(500),
                gst_rate_pct: dec!(0),
                hsn_sac_code: None,
                gst_amount: dec!(0),
            },
            InvoiceLineItemView {
                description: "Maintenance",
                amount: dec!(4500),
                gst_rate_pct: dec!(18),
                hsn_sac_code: Some("999598"),
                gst_amount: dec!(900),
            },
        ];
        let society = Some(crate::db::SocietyProfile {
            name: "Sample Society CHS Ltd".to_string(),
            registration_number: Some("REG/123/2026".to_string()),
            address: "Plot 1, Sample Road, Mumbai".to_string(),
            state: Some("Maharashtra".to_string()),
            phone: Some("9999999999".to_string()),
        });
        let instructions = Some(crate::db::PaymentInstructions {
            account_holder_name: "Sample Society CHS Ltd".to_string(),
            bank_name: "Sample Bank".to_string(),
            account_number: "1234567890".to_string(),
            ifsc_code: "SAMP0001234".to_string(),
            branch: Some("Sample Branch".to_string()),
            contact_email: Some("billing@sample.com".to_string()),
        });
        let bytes = render_invoice_pdf(
            &invoice,
            &line_items,
            "K",
            "0302",
            &society,
            Some("27AAEAR5655D1ZG"),
            &instructions,
            Some(dec!(21)),
            false,
        );
        let text = extract_all_text(&bytes);

        assert!(text.contains("TAX INVOICE"), "missing title label: {text}");
        assert!(
            text.contains("INV-000001"),
            "missing invoice number: {text}"
        );
        assert!(
            text.contains("SAMPLE SOCIETY CHS LTD"),
            "missing society name: {text}"
        );
        assert!(
            text.contains("27AAEAR5655D1ZG"),
            "missing society GSTIN: {text}"
        );
        assert!(
            text.contains("Maharashtra"),
            "missing society state: {text}"
        );
        assert!(
            text.contains("Water Charges"),
            "missing non-taxable line item: {text}"
        );
        assert!(
            text.contains("Maintenance"),
            "missing taxable line item: {text}"
        );
        assert!(text.contains("999598"), "missing SAC code: {text}");
        assert!(
            text.contains("450.00"),
            "missing half-split SGST/CGST amount: {text}"
        );
        assert!(
            text.contains("Total Current Charges"),
            "missing total: {text}"
        );
        assert!(text.contains("5900.00"), "missing total amount: {text}");
        assert!(
            text.contains("21.00% p.a."),
            "missing interest rate note: {text}"
        );
        assert!(
            text.contains("SAMP0001234"),
            "missing bank IFSC in notes: {text}"
        );
    }

    #[test]
    fn receipt_pdf_contains_every_field_it_claims_to_show() {
        let payment = crate::db::Payment {
            payment_id: crate::PaymentId(uuid::Uuid::new_v4()),
            invoice_id: crate::InvoiceId(uuid::Uuid::new_v4()),
            amount: dec!(5000),
            payment_method: "upi".to_string(),
            reference_number: Some("UPI123456".to_string()),
            recorded_by: 1,
            confirmed_at: chrono::Utc::now(),
        };
        let bytes = render_receipt_pdf("RCT-000001", &payment, "INV-000001", "K", "0302");
        let text = extract_all_text(&bytes);

        assert!(text.contains("PAYMENT RECEIPT"), "missing title: {text}");
        assert!(text.contains("K0302"), "missing unit: {text}");
        assert!(
            text.contains("INV-000001"),
            "missing invoice reference: {text}"
        );
        assert!(text.contains("5000.00"), "missing amount received: {text}");
        assert!(
            text.contains("UPI123456"),
            "missing payment reference: {text}"
        );
    }

    #[test]
    fn statement_pdf_contains_every_row_it_claims_to_show() {
        let rows = vec![
            StatementRow {
                date: chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
                description: "Invoice INV-000001".to_string(),
                debit: dec!(5000),
                credit: dec!(0),
                running_balance: dec!(5000),
            },
            StatementRow {
                date: chrono::NaiveDate::from_ymd_opt(2026, 8, 5).unwrap(),
                description: "Payment received".to_string(),
                debit: dec!(0),
                credit: dec!(5000),
                running_balance: dec!(0),
            },
        ];
        let bytes = render_statement_pdf("K", "0302", "August 2026", &rows);
        let text = extract_all_text(&bytes);

        assert!(text.contains("K0302"), "missing unit: {text}");
        assert!(
            text.contains("Invoice INV-000001"),
            "missing first row: {text}"
        );
        assert!(
            text.contains("Payment received"),
            "missing second row: {text}"
        );
        assert!(
            text.contains("Closing Balance"),
            "missing closing balance: {text}"
        );
    }

    /// A non-ASCII description must not panic PDF generation (used to slice by byte index).
    #[test]
    fn statement_pdf_survives_a_non_ascii_multi_byte_description() {
        // `truncate` cuts at 34 chars; this é lands right on the old byte-33 cut point.
        let description = format!("{}é{}", "A".repeat(32), "x".repeat(20));
        let rows = vec![StatementRow {
            date: chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
            description,
            debit: dec!(5000),
            credit: dec!(0),
            running_balance: dec!(5000),
        }];
        let bytes = render_statement_pdf("K", "0302", "August 2026", &rows);
        assert!(!bytes.is_empty(), "PDF must render without panicking");
    }

    #[test]
    fn noc_pdf_contains_every_field_it_claims_to_show() {
        let bytes = render_noc_pdf(
            "K",
            "0302",
            Some("Jane Resident"),
            chrono::NaiveDate::from_ymd_opt(2026, 8, 7).unwrap(),
        );
        let text = extract_all_text(&bytes);

        assert!(text.contains("NO OBJECTION"), "missing title: {text}");
        assert!(text.contains("K0302"), "missing unit: {text}");
        assert!(text.contains("Jane Resident"), "missing owner name: {text}");
        assert!(
            text.contains("NO outstanding"),
            "missing dues statement: {text}"
        );
    }

    /// The sample preview must always demonstrate the full table (non-taxable +
    /// taxable + SGST/CGST split), even when nothing is configured yet — this is the
    /// exact regression a real user hit: with no GST rule saved, the preview used to
    /// degenerate to one bare "Maintenance (sample)" line.
    #[test]
    fn sample_invoice_shows_full_layout_even_with_nothing_configured() {
        let bytes = render_sample_invoice_pdf(&None, &None, &None, None);
        let text = extract_all_text(&bytes);

        assert!(
            text.contains("SAMPLE PREVIEW"),
            "missing sample banner: {text}"
        );
        assert!(
            text.contains("Water Charges"),
            "missing non-taxable line item: {text}"
        );
        assert!(
            text.contains("Sub Total (Non-Taxable)"),
            "missing non-taxable subtotal: {text}"
        );
        assert!(
            text.contains("Maintenance Charges"),
            "missing taxable line item: {text}"
        );
        assert!(
            text.contains("Sub Total (Taxable Services)"),
            "missing taxable subtotal: {text}"
        );
        assert!(text.contains("SGST"), "missing SGST row: {text}");
        assert!(text.contains("CGST"), "missing CGST row: {text}");
        assert!(
            text.contains("Total Current Charges"),
            "missing total: {text}"
        );
    }

    /// When a real GST rule exists, the sample preview must use its actual rate/SAC/
    /// GSTIN instead of the fallback figures — the whole point of "preview" is to show
    /// what's actually configured, not a fixed demo unrelated to real settings.
    #[test]
    fn sample_invoice_uses_the_real_gst_rule_when_one_is_configured() {
        let gst_rule = Some(crate::db::GstRule {
            gst_rule_id: crate::GstRuleId(1),
            per_member_monthly_exempt_threshold: dec!(1000),
            society_turnover_registration_threshold: dec!(2000000),
            gst_rate_pct: dec!(12),
            hsn_sac_code: "997212".to_string(),
            society_gstin: Some("27ABCDE1234F1Z5".to_string()),
            created_by: 1,
        });
        let bytes = render_sample_invoice_pdf(&gst_rule, &None, &None, None);
        let text = extract_all_text(&bytes);

        assert!(text.contains("12.00%"), "missing real GST rate: {text}");
        assert!(text.contains("997212"), "missing real SAC code: {text}");
        assert!(
            text.contains("27ABCDE1234F1Z5"),
            "missing real GSTIN: {text}"
        );
    }
}
