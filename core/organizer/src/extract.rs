//! Document profiles and field extraction.
//!
//! "Training" here means a deterministic profile per document type: a list of
//! canonical fields with their Arabic and English labels plus the aliases that
//! appear on real documents. Nothing is inferred from a language model and
//! nothing is rewritten; a value is only reported when it is present in the
//! recognized text, and an unlabeled guess is always flagged for review.
//!
//! Batch extraction treats **one page as one document**. Rows are emitted in
//! page order and a page with no fields still keeps its row, so a stack of ID
//! cards cannot shift, merge or interleave: row *i* always belongs to page *i*.

use serde::{Deserialize, Serialize};

use crate::model::{Block, BlockKind, Language, OrganizedDocument, Stats, Table, Warning};

/// Bumped when the wire shape changes.
pub const SCHEMA: u32 = 1;

/// A field below this confidence is shown to the user before export.
pub const REVIEW_THRESHOLD: f32 = 0.6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    IdCard,
    Passport,
    Invoice,
    Receipt,
    BankStatement,
    Certificate,
    BusinessCard,
    Letter,
    Form,
    Table,
    Unknown,
}

impl DocumentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IdCard => "id_card",
            Self::Passport => "passport",
            Self::Invoice => "invoice",
            Self::Receipt => "receipt",
            Self::BankStatement => "bank_statement",
            Self::Certificate => "certificate",
            Self::BusinessCard => "business_card",
            Self::Letter => "letter",
            Self::Form => "form",
            Self::Table => "table",
            Self::Unknown => "unknown",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::IdCard => "ID card",
            Self::Passport => "Passport",
            Self::Invoice => "Invoice",
            Self::Receipt => "Receipt",
            Self::BankStatement => "Bank statement",
            Self::Certificate => "Certificate",
            Self::BusinessCard => "Business card",
            Self::Letter => "Letter",
            Self::Form => "Form",
            Self::Table => "Table",
            Self::Unknown => "Document",
        }
    }

    pub fn label_ar(self) -> &'static str {
        match self {
            Self::IdCard => "بطاقة هوية",
            Self::Passport => "جواز سفر",
            Self::Invoice => "فاتورة",
            Self::Receipt => "إيصال",
            Self::BankStatement => "كشف حساب",
            Self::Certificate => "شهادة",
            Self::BusinessCard => "بطاقة عمل",
            Self::Letter => "خطاب",
            Self::Form => "نموذج",
            Self::Table => "جدول",
            Self::Unknown => "مستند",
        }
    }

    /// Parses the value the UI sends when the user picks a profile by hand.
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "id_card" => Self::IdCard,
            "passport" => Self::Passport,
            "invoice" => Self::Invoice,
            "receipt" => Self::Receipt,
            "bank_statement" => Self::BankStatement,
            "certificate" => Self::Certificate,
            "business_card" => Self::BusinessCard,
            "letter" => Self::Letter,
            "form" => Self::Form,
            "table" => Self::Table,
            "unknown" => Self::Unknown,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub key: String,
    /// English label from the profile, or the recognized label for a custom
    /// field.
    pub label: String,
    pub label_ar: String,
    pub value: String,
    pub confidence: f32,
    /// True when the value is an unlabeled guess, a malformed number, or two
    /// recognized values disagree.
    pub needs_review: bool,
    pub source_index: usize,
    pub page: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Extraction {
    pub schema: u32,
    pub kind: DocumentKind,
    pub confidence: f32,
    pub language: Language,
    pub fields: Vec<Field>,
    pub warnings: Vec<Warning>,
}

impl Extraction {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn field(&self, key: &str) -> Option<&Field> {
        self.fields.iter().find(|field| field.key == key)
    }

    /// The fields as a two-column table. Header labels follow the document
    /// language, and every value is the recognized (or user-edited) string.
    pub fn as_table(&self) -> Table {
        let arabic = matches!(self.language, Language::Arabic | Language::Mixed);
        let header = if arabic {
            vec!["الحقل".to_string(), "القيمة".to_string()]
        } else {
            vec!["Field".to_string(), "Value".to_string()]
        };
        let rows: Vec<Vec<String>> = self
            .fields
            .iter()
            .map(|field| {
                vec![
                    if arabic {
                        field.label_ar.clone()
                    } else {
                        field.label.clone()
                    },
                    field.value.clone(),
                ]
            })
            .collect();
        let filled = rows
            .iter()
            .flat_map(|row| row.iter())
            .filter(|value| !value.trim().is_empty())
            .count();
        let cells = rows.len() * 2;
        Table {
            columns: 2,
            header,
            rows,
            confidence: if cells == 0 {
                0.0
            } else {
                filled as f32 / cells as f32
            },
            needs_review: self.fields.iter().any(|field| field.needs_review),
        }
    }

    /// One block list, so a single document reuses the batch serializers.
    pub fn to_document(&self) -> OrganizedDocument {
        let arabic = matches!(self.language, Language::Arabic | Language::Mixed);
        let title = Block {
            kind: BlockKind::Title,
            text: if arabic {
                self.kind.label_ar().to_string()
            } else {
                self.kind.label().to_string()
            },
            level: Some(1),
            items: Vec::new(),
            key: None,
            value: None,
            table: None,
            confidence: self.confidence,
            source_index: 0,
            page: 0,
        };
        let table = Block {
            kind: BlockKind::Table,
            text: String::new(),
            level: None,
            items: Vec::new(),
            key: None,
            value: None,
            table: Some(self.as_table()),
            confidence: self.confidence,
            source_index: 1,
            page: 0,
        };
        let blocks = vec![title, table];
        let stats: Stats = crate::detect::stats(&blocks);
        OrganizedDocument {
            schema: crate::detect::SCHEMA,
            pages: 1,
            language: self.language,
            blocks,
            stats,
            warnings: self.warnings.clone(),
        }
    }

    pub fn to_csv(&self) -> String {
        crate::render::csv(&self.to_document())
    }

    pub fn to_markdown(&self) -> String {
        crate::render::markdown(&self.to_document())
    }

    pub fn to_html(&self) -> String {
        crate::render::html(&self.to_document())
    }

    pub fn plain_text(&self) -> String {
        crate::render::text(&self.to_document())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatchColumn {
    pub key: String,
    pub label: String,
    pub label_ar: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatchRow {
    /// Page this row came from. Never renumbered: row order is page order.
    pub page: usize,
    /// One value per column, in column order.
    pub values: Vec<String>,
    pub confidence: f32,
    pub needs_review: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatchExtraction {
    pub schema: u32,
    pub kind: DocumentKind,
    pub confidence: f32,
    pub language: Language,
    pub columns: Vec<BatchColumn>,
    pub rows: Vec<BatchRow>,
    pub warnings: Vec<Warning>,
}

impl BatchExtraction {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// The batch as a table. Header labels follow the document language so an
    /// Arabic export is Arabic end to end.
    pub fn as_table(&self) -> Table {
        let arabic = matches!(self.language, Language::Arabic | Language::Mixed);
        let header: Vec<String> = self
            .columns
            .iter()
            .map(|column| {
                if arabic {
                    column.label_ar.clone()
                } else {
                    column.label.clone()
                }
            })
            .collect();
        let rows: Vec<Vec<String>> = self.rows.iter().map(|row| row.values.clone()).collect();
        let filled = rows
            .iter()
            .flat_map(|row| row.iter())
            .filter(|value| !value.trim().is_empty())
            .count();
        let cells = rows.len() * header.len().max(1);
        let confidence = if cells == 0 {
            0.0
        } else {
            filled as f32 / cells as f32
        };
        Table {
            columns: header.len(),
            header,
            rows,
            confidence,
            needs_review: self.rows.iter().any(|row| row.needs_review),
        }
    }

    /// One block list, so batch export reuses the same serializers as every
    /// other organized document.
    pub fn to_document(&self) -> OrganizedDocument {
        let arabic = matches!(self.language, Language::Arabic | Language::Mixed);
        let title = Block {
            kind: BlockKind::Title,
            text: if arabic {
                self.kind.label_ar().to_string()
            } else {
                self.kind.label().to_string()
            },
            level: Some(1),
            items: Vec::new(),
            key: None,
            value: None,
            table: None,
            confidence: self.confidence,
            source_index: 0,
            page: 0,
        };
        let table = Block {
            kind: BlockKind::Table,
            text: String::new(),
            level: None,
            items: Vec::new(),
            key: None,
            value: None,
            table: Some(self.as_table()),
            confidence: self.confidence,
            source_index: 1,
            page: 0,
        };
        let blocks = vec![title, table];
        let stats: Stats = crate::detect::stats(&blocks);
        let language = crate::detect::detect_language(&blocks);
        OrganizedDocument {
            schema: crate::detect::SCHEMA,
            pages: 1,
            language,
            blocks,
            stats,
            warnings: self.warnings.clone(),
        }
    }

    pub fn to_csv(&self) -> String {
        crate::render::csv(&self.to_document())
    }

    pub fn to_markdown(&self) -> String {
        crate::render::markdown(&self.to_document())
    }
}

// ------------------------------------------------------------ field specs ---

struct FieldSpec {
    key: &'static str,
    label: &'static str,
    label_ar: &'static str,
    /// Folded with [`fold`]: lowercase, unified alef/ya/ta-marbuta, no harakat.
    aliases: &'static [&'static str],
}

const SPECS: &[FieldSpec] = &[
    FieldSpec {
        key: "full_name",
        label: "Name",
        label_ar: "الاسم",
        aliases: &[
            "name",
            "full name",
            "fullname",
            "holder name",
            "cardholder",
            "card holder",
            "cardholder name",
            "customer name",
            "employee name",
            "student name",
            "beneficiary name",
            "bearer name",
            "name of holder",
            "الاسم",
            "الاسم الكامل",
            "الاسم رباعي",
            "اسم",
            "اسم صاحب البطاقه",
            "اسم حامل البطاقه",
            "اسم العميل",
            "اسم الموظف",
            "اسم الطالب",
            "اسم المستفيد",
        ],
    },
    FieldSpec {
        key: "national_id",
        label: "National ID",
        label_ar: "الرقم الوطني",
        aliases: &[
            "national id",
            "national number",
            "national identification number",
            "id number",
            "id no",
            "id no.",
            "identity number",
            "identity no",
            "personal number",
            "personal id",
            "civil number",
            "civil id",
            "nin",
            "ssn",
            "الرقم الوطني",
            "الرقم القومي",
            "الرقم القومى",
            "رقم البطاقه",
            "رقم الهويه",
            "الرقم الشخصي",
            "الرقم المدني",
            "رقم السجل المدني",
            "رقم التعريف",
        ],
    },
    FieldSpec {
        key: "passport_number",
        label: "Passport number",
        label_ar: "رقم الجواز",
        aliases: &[
            "passport number",
            "passport no",
            "passport no.",
            "passport",
            "رقم الجواز",
            "رقم جواز السفر",
            "جواز السفر",
        ],
    },
    FieldSpec {
        key: "date_of_birth",
        label: "Date of birth",
        label_ar: "تاريخ الميلاد",
        aliases: &[
            "date of birth",
            "birth date",
            "birthdate",
            "dob",
            "born",
            "birthday",
            "تاريخ الميلاد",
            "الميلاد",
            "تاريخ ميلاد",
        ],
    },
    FieldSpec {
        key: "place_of_birth",
        label: "Place of birth",
        label_ar: "محل الميلاد",
        aliases: &[
            "place of birth",
            "birth place",
            "birthplace",
            "محل الميلاد",
            "مكان الميلاد",
        ],
    },
    FieldSpec {
        key: "issue_date",
        label: "Issue date",
        label_ar: "تاريخ الإصدار",
        aliases: &[
            "issue date",
            "date of issue",
            "issued",
            "issued on",
            "تاريخ الاصدار",
            "تاريخ الإصدار",
            "تاريخ الاصدار",
        ],
    },
    FieldSpec {
        key: "expiry_date",
        label: "Expiry date",
        label_ar: "تاريخ الانتهاء",
        aliases: &[
            "expiry date",
            "expiration date",
            "expires",
            "valid until",
            "valid to",
            "date of expiry",
            "تاريخ الانتهاء",
            "تاريخ انتهاء الصلاحيه",
            "صالحه حتي",
            "ينتهي في",
        ],
    },
    FieldSpec {
        key: "gender",
        label: "Gender",
        label_ar: "الجنس",
        aliases: &["gender", "sex", "الجنس"],
    },
    FieldSpec {
        key: "nationality",
        label: "Nationality",
        label_ar: "الجنسية",
        aliases: &["nationality", "citizenship", "الجنسيه", "الجنسية"],
    },
    FieldSpec {
        key: "address",
        label: "Address",
        label_ar: "العنوان",
        aliases: &[
            "address",
            "address line",
            "residence",
            "home address",
            "العنوان",
            "محل الاقامه",
            "العنوان الوطني",
            "السكن",
        ],
    },
    FieldSpec {
        key: "phone",
        label: "Phone",
        label_ar: "الهاتف",
        aliases: &[
            "phone",
            "phone number",
            "phone no",
            "mobile",
            "mobile number",
            "tel",
            "telephone",
            "whatsapp",
            "الهاتف",
            "رقم الهاتف",
            "الجوال",
            "رقم الجوال",
            "الموبايل",
            "موبايل",
            "تليفون",
        ],
    },
    FieldSpec {
        key: "email",
        label: "Email",
        label_ar: "البريد الإلكتروني",
        aliases: &[
            "email",
            "email address",
            "e-mail",
            "mail",
            "البريد الالكتروني",
            "البريد الإلكتروني",
            "البريد",
            "ايميل",
        ],
    },
    FieldSpec {
        key: "website",
        label: "Website",
        label_ar: "الموقع",
        aliases: &[
            "website",
            "web",
            "site",
            "url",
            "الموقع",
            "الموقع الالكتروني",
        ],
    },
    FieldSpec {
        key: "company",
        label: "Company",
        label_ar: "الشركة",
        aliases: &[
            "company",
            "company name",
            "employer",
            "organization",
            "organisation",
            "firm",
            "الشركه",
            "المؤسسه",
            "جهه العمل",
        ],
    },
    FieldSpec {
        key: "job_title",
        label: "Job title",
        label_ar: "المسمى الوظيفي",
        aliases: &[
            "job title",
            "title",
            "position",
            "role",
            "designation",
            "المسمى الوظيفي",
            "الوظيفه",
            "المنصب",
        ],
    },
    FieldSpec {
        key: "invoice_number",
        label: "Invoice number",
        label_ar: "رقم الفاتورة",
        aliases: &[
            "invoice number",
            "invoice no",
            "invoice no.",
            "invoice #",
            "invoice",
            "bill number",
            "bill no",
            "رقم الفاتوره",
            "فاتوره رقم",
            "رقم الفاتورة",
        ],
    },
    FieldSpec {
        key: "invoice_date",
        label: "Invoice date",
        label_ar: "تاريخ الفاتورة",
        aliases: &["invoice date", "bill date", "تاريخ الفاتوره"],
    },
    FieldSpec {
        key: "date",
        label: "Date",
        label_ar: "التاريخ",
        aliases: &["date", "التاريخ", "تاريخ"],
    },
    FieldSpec {
        key: "vendor",
        label: "Vendor",
        label_ar: "المورد",
        aliases: &[
            "vendor",
            "seller",
            "supplier",
            "from",
            "المورد",
            "البائع",
            "من",
        ],
    },
    FieldSpec {
        key: "merchant",
        label: "Merchant",
        label_ar: "التاجر",
        aliases: &[
            "merchant",
            "merchant name",
            "store",
            "store name",
            "shop",
            "التاجر",
            "المتجر",
            "اسم المتجر",
        ],
    },
    FieldSpec {
        key: "customer",
        label: "Customer",
        label_ar: "العميل",
        aliases: &[
            "customer",
            "client",
            "bill to",
            "sold to",
            "buyer",
            "العميل",
            "المشتري",
            "الزبون",
            "فاتوره الي",
        ],
    },
    FieldSpec {
        key: "subtotal",
        label: "Subtotal",
        label_ar: "المجموع الفرعي",
        aliases: &[
            "subtotal",
            "sub total",
            "net",
            "المجموع الفرعي",
            "الاجمالي قبل الضريبه",
            "الصافي",
        ],
    },
    FieldSpec {
        key: "tax",
        label: "Tax",
        label_ar: "الضريبة",
        aliases: &[
            "tax",
            "tax amount",
            "vat",
            "vat amount",
            "sales tax",
            "ضريبه",
            "الضريبه",
            "ضريبة القيمة المضافة",
        ],
    },
    FieldSpec {
        key: "total",
        label: "Total",
        label_ar: "الإجمالي",
        aliases: &[
            "total",
            "total amount",
            "grand total",
            "amount due",
            "total due",
            "balance due",
            "الاجمالي",
            "المجموع",
            "الاجمالي النهائي",
            "المبلغ المستحق",
            "المبلغ الاجمالي",
        ],
    },
    FieldSpec {
        key: "currency",
        label: "Currency",
        label_ar: "العملة",
        aliases: &["currency", "العملة"],
    },
    FieldSpec {
        key: "payment_method",
        label: "Payment method",
        label_ar: "طريقة الدفع",
        aliases: &[
            "payment method",
            "paid by",
            "payment",
            "طريقة الدفع",
            "الدفع",
        ],
    },
    FieldSpec {
        key: "receipt_number",
        label: "Receipt number",
        label_ar: "رقم الإيصال",
        aliases: &[
            "receipt number",
            "receipt no",
            "receipt no.",
            "رقم الايصال",
            "رقم الإيصال",
        ],
    },
    FieldSpec {
        key: "account_holder",
        label: "Account holder",
        label_ar: "صاحب الحساب",
        aliases: &[
            "account holder",
            "account holder name",
            "account name",
            "صاحب الحساب",
            "اسم صاحب الحساب",
        ],
    },
    FieldSpec {
        key: "account_number",
        label: "Account number",
        label_ar: "رقم الحساب",
        aliases: &[
            "account number",
            "account no",
            "account no.",
            "a/c",
            "رقم الحساب",
        ],
    },
    FieldSpec {
        key: "iban",
        label: "IBAN",
        label_ar: "IBAN",
        aliases: &["iban", "ايبان", "الايبان"],
    },
    FieldSpec {
        key: "swift",
        label: "SWIFT",
        label_ar: "SWIFT",
        aliases: &["swift", "swift code", "bic"],
    },
    FieldSpec {
        key: "opening_balance",
        label: "Opening balance",
        label_ar: "الرصيد الافتتاحي",
        aliases: &[
            "opening balance",
            "balance brought forward",
            "الرصيد الافتتاحي",
        ],
    },
    FieldSpec {
        key: "closing_balance",
        label: "Closing balance",
        label_ar: "الرصيد الختامي",
        aliases: &[
            "closing balance",
            "balance carried forward",
            "الرصيد الختامي",
            "الرصيد",
        ],
    },
    FieldSpec {
        key: "statement_date",
        label: "Statement date",
        label_ar: "تاريخ الكشف",
        aliases: &["statement date", "statement period", "تاريخ الكشف"],
    },
    FieldSpec {
        key: "certificate_title",
        label: "Certificate",
        label_ar: "الشهادة",
        aliases: &["certificate", "certificate of", "الشهاده", "شهادة"],
    },
    FieldSpec {
        key: "certificate_number",
        label: "Certificate number",
        label_ar: "رقم الشهادة",
        aliases: &[
            "certificate number",
            "certificate no",
            "certificate no.",
            "رقم الشهاده",
            "رقم الشهادة",
        ],
    },
    FieldSpec {
        key: "issuer",
        label: "Issued by",
        label_ar: "الجهة المصدرة",
        aliases: &[
            "issuer",
            "issued by",
            "authority",
            "الجهه المصدره",
            "جهة الإصدار",
            "المصدر",
        ],
    },
    FieldSpec {
        key: "holder_name",
        label: "Holder",
        label_ar: "اسم الحاصل",
        aliases: &["holder", "holder name", "اسم الحاصل", "الحاصل علي"],
    },
    FieldSpec {
        key: "sender",
        label: "Sender",
        label_ar: "المرسل",
        aliases: &["sender", "from", "المرسل", "من"],
    },
    FieldSpec {
        key: "recipient",
        label: "Recipient",
        label_ar: "المستلم",
        aliases: &["recipient", "to", "addressed to", "الي", "المستلم"],
    },
    FieldSpec {
        key: "subject",
        label: "Subject",
        label_ar: "الموضوع",
        aliases: &["subject", "re", "regarding", "الموضوع", "بخصوص"],
    },
    FieldSpec {
        key: "reference",
        label: "Reference",
        label_ar: "المرجع",
        aliases: &[
            "reference",
            "ref",
            "ref no",
            "reference number",
            "المرجع",
            "رقم المرجع",
        ],
    },
];

fn spec_for(key: &str) -> Option<&'static FieldSpec> {
    SPECS.iter().find(|spec| spec.key == key)
}

fn profile(kind: DocumentKind) -> &'static [&'static str] {
    match kind {
        DocumentKind::IdCard => &[
            "full_name",
            "national_id",
            "date_of_birth",
            "gender",
            "nationality",
            "address",
            "issue_date",
            "expiry_date",
        ],
        DocumentKind::Passport => &[
            "full_name",
            "passport_number",
            "nationality",
            "date_of_birth",
            "place_of_birth",
            "gender",
            "issue_date",
            "expiry_date",
        ],
        DocumentKind::Invoice => &[
            "invoice_number",
            "invoice_date",
            "vendor",
            "customer",
            "subtotal",
            "tax",
            "total",
            "currency",
            "payment_method",
        ],
        DocumentKind::Receipt => &[
            "merchant",
            "receipt_number",
            "date",
            "total",
            "currency",
            "payment_method",
        ],
        DocumentKind::BankStatement => &[
            "account_holder",
            "account_number",
            "iban",
            "statement_date",
            "opening_balance",
            "closing_balance",
            "currency",
        ],
        DocumentKind::Certificate => &[
            "certificate_title",
            "holder_name",
            "certificate_number",
            "issuer",
            "issue_date",
        ],
        DocumentKind::BusinessCard => &[
            "full_name",
            "job_title",
            "company",
            "phone",
            "email",
            "website",
            "address",
        ],
        DocumentKind::Letter => &["sender", "recipient", "date", "subject", "reference"],
        DocumentKind::Form | DocumentKind::Table | DocumentKind::Unknown => &[],
    }
}

// -------------------------------------------------------------- detection ---

/// Folds text for matching only. The stored value is never folded.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        let mapped = match ch {
            '\u{0640}' | '\u{064B}'..='\u{065F}' | '\u{0670}' => continue,
            '\u{0623}' | '\u{0625}' | '\u{0622}' | '\u{0671}' => '\u{0627}',
            '\u{0649}' => '\u{064A}',
            '\u{0629}' => '\u{0647}',
            _ => ch,
        };
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        for lower in mapped.to_lowercase() {
            out.push(lower);
        }
    }
    out.trim_matches(|ch: char| matches!(ch, ':' | '*' | '-' | '.' | '#'))
        .trim()
        .to_string()
}

fn match_spec(label: &str) -> Option<&'static FieldSpec> {
    let folded = fold(label);
    if folded.is_empty() {
        return None;
    }
    let mut best: Option<(&'static FieldSpec, bool, usize)> = None;
    for spec in SPECS {
        for alias in spec.aliases {
            let exact = folded == *alias;
            let length = alias.chars().count();
            // A short generic alias ("payment", "name", "date") must match
            // exactly; only a specific alias may be the start of a longer label.
            let can_prefix = alias.split_whitespace().count() >= 2 || length >= 9;
            let prefix = !exact && can_prefix && folded.starts_with(alias);
            if !exact && !prefix {
                continue;
            }
            let extra = if prefix {
                folded[alias.len()..].split_whitespace().count()
            } else {
                0
            };
            // "Name of bank" is not a person's name.
            if prefix && (extra == 0 || extra > 2) {
                continue;
            }
            let better = match best {
                None => true,
                Some((_, best_exact, best_length)) => (exact, length) > (best_exact, best_length),
            };
            if better {
                best = Some((spec, exact, length));
            }
        }
    }
    best.map(|(spec, _, _)| spec)
}

fn block_text(block: &Block) -> String {
    let mut text = block.text.clone();
    for item in &block.items {
        text.push('\n');
        text.push_str(item);
    }
    if let Some(value) = &block.value {
        text.push(' ');
        text.push_str(value);
    }
    if let Some(table) = &block.table {
        for row in table.all_rows() {
            text.push('\n');
            text.push_str(&row.join(" "));
        }
    }
    text
}

fn document_text(blocks: &[&Block]) -> String {
    blocks
        .iter()
        .map(|block| block_text(block))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Keyword scores per profile. Every keyword is already folded.
fn keyword_score(kind: DocumentKind, text: &str) -> f32 {
    let words: &[(&str, f32)] = match kind {
        DocumentKind::IdCard => &[
            ("بطاقه تحقيق الشخصيه", 3.0),
            ("بطاقه هويه", 3.0),
            ("بطاقة هوية", 3.0),
            ("national id", 2.5),
            ("identity card", 2.5),
            ("id card", 2.0),
            ("رقم قومي", 2.0),
            ("الرقم الوطني", 2.0),
            ("بطاقه", 1.0),
            ("هويه", 1.0),
            ("تاريخ الميلاد", 1.0),
            ("جمهوريه", 0.5),
        ],
        DocumentKind::Passport => &[
            ("passport", 3.0),
            ("جواز السفر", 3.0),
            ("جواز سفر", 3.0),
            ("passport no", 2.0),
            ("رقم الجواز", 2.0),
            ("place of birth", 1.0),
            ("محل الميلاد", 1.0),
        ],
        DocumentKind::Invoice => &[
            ("رقم الفاتوره", 2.5),
            ("فاتوره ضريبيه", 2.5),
            ("tax invoice", 2.5),
            ("invoice number", 2.5),
            ("invoice", 2.0),
            ("فاتوره", 2.0),
            ("bill to", 1.0),
            ("ضريبه", 0.8),
            ("vat", 0.8),
        ],
        DocumentKind::Receipt => &[
            ("ايصال", 2.5),
            ("receipt", 2.5),
            ("طريقة الدفع", 1.0),
            ("payment method", 1.0),
            ("الباقي", 0.8),
            ("cash", 0.6),
            ("نقدا", 0.8),
        ],
        DocumentKind::BankStatement => &[
            ("كشف حساب", 3.0),
            ("bank statement", 3.0),
            ("account statement", 2.5),
            ("iban", 2.0),
            ("رقم الحساب", 1.5),
            ("account number", 1.5),
            ("opening balance", 1.5),
            ("الرصيد الافتتاحي", 1.5),
            ("رصيد", 0.8),
            ("balance", 0.8),
        ],
        DocumentKind::Certificate => &[
            ("شهاده تقدير", 2.5),
            ("شهاده", 2.0),
            ("certificate of", 2.0),
            ("certificate", 2.0),
            ("يشهد", 1.0),
            ("certifies", 1.0),
        ],
        DocumentKind::BusinessCard => &[
            ("@", 1.5),
            ("www.", 1.2),
            ("المسمى الوظيفي", 1.5),
            ("job title", 1.5),
            ("هاتف", 0.8),
            ("phone", 0.8),
            ("شركه", 0.8),
            ("company", 0.8),
        ],
        DocumentKind::Letter => &[
            ("تحيه طيبه", 1.5),
            ("وتفضلوا بقبول", 1.5),
            ("الموضوع", 1.2),
            ("dear", 1.5),
            ("sincerely", 1.5),
            ("subject", 1.0),
            ("regarding", 0.8),
        ],
        DocumentKind::Form | DocumentKind::Table | DocumentKind::Unknown => &[],
    };

    let mut score = 0.0;
    for (word, weight) in words {
        if text.contains(word) {
            score += weight;
        }
    }
    score
}

/// Picks the document profile. The score is keyword evidence, never a promise:
/// the returned confidence says how sure the detector is.
pub fn detect_kind(blocks: &[&Block]) -> (DocumentKind, f32) {
    let text = fold(&document_text(blocks));
    let has_table = blocks.iter().any(|block| block.table.is_some());
    let key_values = blocks
        .iter()
        .filter(|block| block.kind == BlockKind::KeyValue)
        .count();

    let mut best = (DocumentKind::Unknown, 0.0f32);
    for kind in [
        DocumentKind::IdCard,
        DocumentKind::Passport,
        DocumentKind::Invoice,
        DocumentKind::Receipt,
        DocumentKind::BankStatement,
        DocumentKind::Certificate,
        DocumentKind::BusinessCard,
        DocumentKind::Letter,
    ] {
        let score = keyword_score(kind, &text);
        if score > best.1 {
            best = (kind, score);
        }
    }

    if key_values >= 3 {
        let form_score = (key_values as f32 * 0.45).min(3.0);
        if form_score > best.1 {
            best = (DocumentKind::Form, form_score);
        }
    }

    // An unlabeled card is still recognizable: a 14-digit number whose first
    // digit is a century and whose next four digits are a real date is an
    // Egyptian national ID, not an arbitrary number.
    let looks_like_national_id = id_candidates(&text)
        .iter()
        .any(|(_, digits)| digits.len() == 14 && egyptian_id_valid(digits));
    if looks_like_national_id && best.1 < 2.0 {
        best = (DocumentKind::IdCard, 2.0);
    }

    if best.1 < 1.5 {
        return if has_table {
            (DocumentKind::Table, 0.55)
        } else {
            (DocumentKind::Unknown, 0.2)
        };
    }

    let confidence = (0.35 + best.1 * 0.12).clamp(0.0, 0.95);
    (best.0, confidence)
}

// -------------------------------------------------------------- extraction --

struct Pair {
    /// `Some` only when the label matched a profile alias. A custom label is
    /// never re-matched later, so "Payment terms" cannot become "Payment".
    spec: Option<&'static FieldSpec>,
    custom_key: String,
    value: String,
    source_index: usize,
    confidence: f32,
    page: usize,
}

fn table_pairs(block: &Block, out: &mut Vec<Pair>) {
    let Some(table) = &block.table else {
        return;
    };
    for row in table.all_rows() {
        if row.len() < 2 {
            continue;
        }
        let Some(spec) = match_spec(&row[0]) else {
            continue;
        };
        let value = row[1..]
            .iter()
            .map(|cell| cell.trim())
            .filter(|cell| !cell.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if value.is_empty() {
            continue;
        }
        out.push(Pair {
            spec: Some(spec),
            custom_key: String::new(),
            value,
            source_index: block.source_index,
            confidence: block.confidence.min(0.9),
            page: block.page,
        });
    }
}

fn labeled_pairs(blocks: &[&Block]) -> Vec<Pair> {
    let mut pairs = Vec::new();
    for block in blocks {
        match block.kind {
            BlockKind::KeyValue => {
                if let (Some(key), Some(value)) = (&block.key, &block.value) {
                    if value.trim().is_empty() {
                        continue;
                    }
                    let spec = match_spec(key);
                    let custom_key = if spec.is_some() {
                        String::new()
                    } else {
                        format!("custom:{}", fold(key).replace(' ', "_"))
                    };
                    pairs.push(Pair {
                        spec,
                        custom_key,
                        value: value.clone(),
                        source_index: block.source_index,
                        confidence: block.confidence,
                        page: block.page,
                    });
                }
            }
            BlockKind::Table => table_pairs(block, &mut pairs),
            _ => {}
        }
    }
    pairs
}

fn field_for(spec: &'static FieldSpec, pair: &Pair) -> Field {
    let (value, needs_review) = if spec.key == "national_id" {
        let (value, review) = national_id_state(&pair.value);
        (value, review)
    } else {
        (pair.value.trim().to_string(), false)
    };
    Field {
        key: spec.key.to_string(),
        label: spec.label.to_string(),
        label_ar: spec.label_ar.to_string(),
        value,
        confidence: pair.confidence,
        needs_review,
        source_index: pair.source_index,
        page: pair.page,
    }
}

fn custom_field(pair: &Pair) -> Field {
    let raw = pair.custom_key.trim_start_matches("custom:");
    let label = raw.replace('_', " ");
    Field {
        key: pair.custom_key.clone(),
        label: label.clone(),
        label_ar: label,
        value: pair.value.trim().to_string(),
        confidence: pair.confidence * 0.8,
        needs_review: true,
        source_index: pair.source_index,
        page: pair.page,
    }
}

fn national_id_state(value: &str) -> (String, bool) {
    let digits: String = value.chars().filter_map(ascii_digit).collect();
    match digits.len() {
        14 if egyptian_id_valid(&digits) => (value.trim().to_string(), false),
        14 => (value.trim().to_string(), true),
        10 if digits.starts_with('1') || digits.starts_with('2') => {
            (value.trim().to_string(), false)
        }
        8..=16 => (value.trim().to_string(), true),
        _ => (value.trim().to_string(), true),
    }
}

fn egyptian_id_valid(digits: &str) -> bool {
    let bytes = digits.as_bytes();
    if bytes.len() != 14 {
        return false;
    }
    if bytes[0] != b'2' && bytes[0] != b'3' {
        return false;
    }
    let month: u32 = digits[3..5].parse().unwrap_or(0);
    let day: u32 = digits[5..7].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn ascii_digit(ch: char) -> Option<char> {
    match ch {
        '0'..='9' => Some(ch),
        '\u{0660}'..='\u{0669}' => char::from_u32(ch as u32 - 0x0660 + '0' as u32),
        '\u{06F0}'..='\u{06F9}' => char::from_u32(ch as u32 - 0x06F0 + '0' as u32),
        _ => None,
    }
}

/// Finds digit runs of 8..=16 digits, including runs split by spaces or dashes
/// ("1234 5678 9012 34"). Returns the original spelling and the digits.
fn id_candidates(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for line in text.lines() {
        if line.contains('/') {
            // A slash-separated run is a date, not an identifier.
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let mut index = 0;
        while index < words.len() {
            let start = index;
            let mut digits = String::new();
            while index < words.len() {
                let word_digits: String = words[index]
                    .chars()
                    .filter_map(|ch| ascii_digit(ch).or(if ch == '-' { Some('-') } else { None }))
                    .collect();
                let numeric = !word_digits.is_empty()
                    && word_digits
                        .chars()
                        .all(|ch| ch.is_ascii_digit() || ch == '-');
                if !numeric {
                    break;
                }
                digits.extend(word_digits.chars().filter(|ch| ch.is_ascii_digit()));
                index += 1;
            }
            if index == start {
                index += 1;
                continue;
            }
            if (10..=16).contains(&digits.len()) {
                found.push((words[start..index].join(" "), digits));
            }
        }
    }
    found
}

fn name_candidate(blocks: &[&Block]) -> Option<(String, usize, f32)> {
    const STOP_WORDS: [&str; 14] = [
        "جمهوريه",
        "وزاره",
        "الداخليه",
        "بطاقه",
        "تحقيق",
        "الشخصيه",
        "ministry",
        "republic",
        "identity",
        "card",
        "passport",
        "address",
        "العنوان",
        "شارع",
    ];
    let mut fallback: Option<(String, usize, f32)> = None;
    for block in blocks {
        if !matches!(block.kind, BlockKind::Title | BlockKind::Paragraph) {
            continue;
        }
        let text = block.text.trim();
        if text.is_empty() || text.contains('@') || text.contains("://") {
            continue;
        }
        let words = text.split_whitespace().count();
        if !(2..=6).contains(&words) {
            continue;
        }
        if text.chars().any(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let folded = fold(text);
        if STOP_WORDS.iter().any(|word| folded.contains(word)) {
            continue;
        }
        if match_spec(text).is_some() {
            continue;
        }
        let confidence = if block.kind == BlockKind::Title {
            0.6
        } else {
            0.5
        };
        let candidate = (text.to_string(), block.source_index, confidence);
        if block.kind == BlockKind::Title {
            return Some(candidate);
        }
        if fallback.is_none() {
            fallback = Some(candidate);
        }
    }
    fallback
}

fn email_candidate(text: &str) -> Option<String> {
    for token in text.split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';') {
        let token = token.trim_matches(|ch: char| ch == '<' || ch == '>' || ch == '.');
        let mut parts = token.split('@');
        let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if local.len() >= 2 && domain.contains('.') && !domain.starts_with('.') {
            return Some(token.to_string());
        }
    }
    None
}

fn phone_candidate(text: &str) -> Option<String> {
    for line in text.lines() {
        let digits = line.chars().filter_map(ascii_digit).count();
        let letters = line.chars().filter(|ch| ch.is_alphabetic()).count();
        if (7..=15).contains(&digits) && letters <= 3 && !line.contains('@') {
            if let Some((original, _)) = id_candidates(line).into_iter().next()
                && original.chars().filter_map(ascii_digit).count() > 15
            {
                continue;
            }
            return Some(line.trim().to_string());
        }
    }
    None
}

fn insert(fields: &mut Vec<Field>, field: Field, warnings: &mut Vec<Warning>) {
    if let Some(existing) = fields.iter_mut().find(|item| item.key == field.key) {
        if existing.value == field.value {
            if field.confidence > existing.confidence {
                existing.confidence = field.confidence;
            }
            existing.needs_review |= field.needs_review;
            return;
        }
        if field.confidence > existing.confidence + 0.15 {
            warnings.push(Warning {
                code: "ambiguous_field".to_string(),
                message: format!(
                    "Two values were recognized for {}: \"{}\" and \"{}\". The more confident one was kept.",
                    existing.label, existing.value, field.value
                ),
                block: Some(existing.source_index),
            });
            *existing = field;
            existing.needs_review = true;
        } else {
            warnings.push(Warning {
                code: "ambiguous_field".to_string(),
                message: format!(
                    "Two values were recognized for {}: \"{}\" and \"{}\". Check this field.",
                    existing.label, existing.value, field.value
                ),
                block: Some(existing.source_index),
            });
            existing.needs_review = true;
        }
        return;
    }
    fields.push(field);
}

fn extract_blocks(
    kind: DocumentKind,
    kind_confidence: f32,
    language: Language,
    blocks: &[&Block],
) -> Extraction {
    let mut warnings = Vec::new();
    let mut found: Vec<Field> = Vec::new();

    for pair in labeled_pairs(blocks) {
        match pair.spec {
            Some(spec) => insert(&mut found, field_for(spec, &pair), &mut warnings),
            None => insert(&mut found, custom_field(&pair), &mut warnings),
        }
    }

    let text = blocks
        .iter()
        .map(|block| block_text(block))
        .collect::<Vec<_>>()
        .join("\n");
    let free: Vec<&Block> = blocks
        .iter()
        .copied()
        .filter(|block| {
            !matches!(
                block.kind,
                BlockKind::KeyValue | BlockKind::Table | BlockKind::Figure
            )
        })
        .collect();

    let has = |fields: &[Field], key: &str| fields.iter().any(|field| field.key == key);

    // Unlabeled numbers. Only a profile that has the field accepts it.
    let wants_id = profile(kind).contains(&"national_id") && !has(&found, "national_id");
    if wants_id && let Some((original, digits)) = id_candidates(&text).into_iter().next() {
        let review = national_id_state(&digits).1;
        let confidence = match digits.len() {
            14 if !review => 0.9,
            10 if !review => 0.85,
            _ => 0.5,
        };
        insert(
            &mut found,
            Field {
                key: "national_id".to_string(),
                label: "National ID".to_string(),
                label_ar: "الرقم الوطني".to_string(),
                value: original,
                confidence,
                needs_review: true,
                source_index: 0,
                page: blocks.first().map(|block| block.page).unwrap_or(0),
            },
            &mut warnings,
        );
    }

    if profile(kind).contains(&"full_name")
        && !has(&found, "full_name")
        && let Some((name, source_index, confidence)) = name_candidate(&free)
    {
        insert(
            &mut found,
            Field {
                key: "full_name".to_string(),
                label: "Name".to_string(),
                label_ar: "الاسم".to_string(),
                value: name,
                confidence,
                needs_review: true,
                source_index,
                page: blocks.first().map(|block| block.page).unwrap_or(0),
            },
            &mut warnings,
        );
    }

    if !has(&found, "email")
        && let Some(email) = email_candidate(&text)
        && let Some(spec) = spec_for("email")
    {
        insert(
            &mut found,
            Field {
                key: spec.key.to_string(),
                label: spec.label.to_string(),
                label_ar: spec.label_ar.to_string(),
                value: email,
                confidence: 0.85,
                needs_review: false,
                source_index: 0,
                page: blocks.first().map(|block| block.page).unwrap_or(0),
            },
            &mut warnings,
        );
    }

    if profile(kind).contains(&"phone")
        && !has(&found, "phone")
        && let Some(phone) = phone_candidate(&text)
    {
        insert(
            &mut found,
            Field {
                key: "phone".to_string(),
                label: "Phone".to_string(),
                label_ar: "الهاتف".to_string(),
                value: phone,
                confidence: 0.6,
                needs_review: true,
                source_index: 0,
                page: blocks.first().map(|block| block.page).unwrap_or(0),
            },
            &mut warnings,
        );
    }

    // Profile order first, then custom fields in the order they were seen.
    let mut ordered: Vec<Field> = Vec::with_capacity(found.len());
    for key in profile(kind) {
        if let Some(position) = found.iter().position(|field| field.key == *key) {
            ordered.push(found.remove(position));
        }
    }
    ordered.append(&mut found);

    if ordered.is_empty() {
        warnings.push(Warning {
            code: "no_fields".to_string(),
            message:
                "No labeled fields were found. The text is still available in the organized view."
                    .to_string(),
            block: None,
        });
    }

    for (key, code) in [
        ("national_id", "national_id_missing"),
        ("full_name", "name_missing"),
    ] {
        if profile(kind).contains(&key) && !ordered.iter().any(|field| field.key == key) {
            warnings.push(Warning {
                code: code.to_string(),
                message: format!(
                    "This {} has no {} field. Add it by hand before exporting if it is present.",
                    kind.label(),
                    spec_for(key).map(|spec| spec.label).unwrap_or(key)
                ),
                block: None,
            });
        }
    }

    if matches!(kind, DocumentKind::Invoice | DocumentKind::Receipt)
        && !ordered.iter().any(|field| field.key == "total")
    {
        warnings.push(Warning {
            code: "total_missing".to_string(),
            message: "No total was recognized. It is not calculated for you; add it if the document has one."
                .to_string(),
            block: None,
        });
    }

    for field in &ordered {
        if field.needs_review {
            warnings.push(Warning {
                code: "field_needs_review".to_string(),
                message: format!("Check {}: \"{}\"", field.label, field.value),
                block: Some(field.source_index),
            });
        }
    }

    Extraction {
        schema: SCHEMA,
        kind,
        confidence: kind_confidence,
        language,
        fields: ordered,
        warnings,
    }
}

/// Extracts one document (all pages of the same document).
pub fn extract(document: &OrganizedDocument) -> Extraction {
    let blocks: Vec<&Block> = document.blocks.iter().collect();
    let (kind, confidence) = detect_kind(&blocks);
    extract_blocks(kind, confidence, document.language, &blocks)
}

/// Extracts with a profile the user picked, so a wrong guess is never final.
pub fn extract_as(document: &OrganizedDocument, kind: DocumentKind) -> Extraction {
    let blocks: Vec<&Block> = document.blocks.iter().collect();
    extract_blocks(kind, 1.0, document.language, &blocks)
}

/// Extracts every page as its own document and returns one table.
///
/// Rows are page-ordered and one row per page is always emitted, so a blank or
/// unreadable card keeps its slot instead of letting the next card move up.
pub fn extract_batch(document: &OrganizedDocument) -> BatchExtraction {
    extract_batch_with(document, None)
}

/// Batch extraction with a profile the user picked by hand.
///
/// Every page is read with that profile, so the table and the single-document
/// view cannot disagree when the detector guessed wrong.
pub fn extract_batch_as(document: &OrganizedDocument, kind: DocumentKind) -> BatchExtraction {
    extract_batch_with(document, Some(kind))
}

fn extract_batch_with(
    document: &OrganizedDocument,
    forced: Option<DocumentKind>,
) -> BatchExtraction {
    let mut warnings = Vec::new();
    let pages = document.pages.max(1);

    let mut per_page: Vec<Extraction> = Vec::with_capacity(pages);
    for page in 0..pages {
        let blocks: Vec<&Block> = document
            .blocks
            .iter()
            .filter(|block| block.page == page)
            .collect();
        let (kind, confidence) = match forced {
            Some(kind) => (kind, 1.0),
            None if blocks.is_empty() => (DocumentKind::Unknown, 0.0),
            None => detect_kind(&blocks),
        };
        let extraction = extract_blocks(kind, confidence, document.language, &blocks);
        per_page.push(extraction);
    }

    // A manual profile is the batch profile. Otherwise it is the most confident
    // page's profile; ties go to the first page so the result is reproducible.
    let (mut kind, mut confidence) = match forced {
        Some(kind) => (kind, 1.0),
        None => (DocumentKind::Unknown, 0.0),
    };
    if forced.is_none() {
        for extraction in &per_page {
            if extraction.confidence > confidence {
                kind = extraction.kind;
                confidence = extraction.confidence;
            }
        }
    }

    let mut columns: Vec<BatchColumn> = profile(kind)
        .iter()
        .filter_map(|key| spec_for(key))
        .map(|spec| BatchColumn {
            key: spec.key.to_string(),
            label: spec.label.to_string(),
            label_ar: spec.label_ar.to_string(),
        })
        .collect();
    let mut seen: Vec<String> = columns.iter().map(|column| column.key.clone()).collect();

    for extraction in &per_page {
        for field in &extraction.fields {
            if seen.contains(&field.key) {
                continue;
            }
            seen.push(field.key.clone());
            columns.push(BatchColumn {
                key: field.key.clone(),
                label: field.label.clone(),
                label_ar: field.label_ar.clone(),
            });
        }
    }

    let mut rows: Vec<BatchRow> = Vec::with_capacity(pages);
    for (page, extraction) in per_page.iter().enumerate() {
        let values: Vec<String> = columns
            .iter()
            .map(|column| {
                extraction
                    .field(&column.key)
                    .map(|field| field.value.clone())
                    .unwrap_or_default()
            })
            .collect();
        let filled = values
            .iter()
            .filter(|value| !value.trim().is_empty())
            .count();
        let mut needs_review = extraction.fields.iter().any(|field| field.needs_review);
        if filled == 0 {
            needs_review = true;
            warnings.push(Warning {
                code: "empty_page".to_string(),
                message: format!(
                    "Page {} produced no fields; its row is kept blank.",
                    page + 1
                ),
                block: None,
            });
        }
        let page_confidence = if columns.is_empty() {
            0.0
        } else {
            filled as f32 / columns.len() as f32
        };
        rows.push(BatchRow {
            page,
            values,
            confidence: page_confidence,
            needs_review,
        });
    }

    // A repeated identifier means the stack has a duplicate or the OCR read one
    // card twice. Both need a human, and neither may be dropped silently.
    for key in ["national_id", "passport_number", "invoice_number"] {
        let Some(column) = columns.iter().position(|column| column.key == key) else {
            continue;
        };
        let mut seen_values: Vec<(String, usize)> = Vec::new();
        for row in &rows {
            let value = row
                .values
                .get(column)
                .map(String::as_str)
                .unwrap_or_default();
            if value.trim().is_empty() {
                continue;
            }
            if let Some((_, first_page)) = seen_values.iter().find(|(seen, _)| seen == value) {
                warnings.push(Warning {
                    code: "duplicate_value".to_string(),
                    message: format!(
                        "The same {} \"{}\" appears on pages {} and {}.",
                        columns[column].label,
                        value,
                        first_page + 1,
                        row.page + 1
                    ),
                    block: Some(row.page),
                });
            } else {
                seen_values.push((value.to_string(), row.page));
            }
        }
    }

    BatchExtraction {
        schema: SCHEMA,
        kind,
        confidence,
        language: document.language,
        columns,
        rows,
        warnings,
    }
}
