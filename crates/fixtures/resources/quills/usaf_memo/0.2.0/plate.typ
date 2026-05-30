#import "@local/quillmark-helper:0.1.0": data, signature-field
#import "@local/tonguetoquill-usaf-memo:3.0.0": backmatter, frontmatter, indorsement, mainmatter

// Frontmatter configuration
#show: frontmatter.with(
  // Letterhead configuration
  letterhead_title: data.letterhead_title,
  letterhead_caption: data.letterhead_caption,
  letterhead_seal_subtitle: data.at("letterhead_seal_subtitle", default: none),
  letterhead_seal: image(
    if data.at("letterhead_seal", default: "dow") == "dod" {
      "assets/dod_seal.png"
    } else {
      "assets/dow_seal.png"
    }
  ),

  // Optional Freedom 250 emblem, placed opposite the seal
  ..if data.at("freedom250", default: false) {
    (letterhead_emblem: image("assets/freedom250.png"))
  },

  // Date
  date: data.at("date", default: none),

  // Receiver information
  memo_for: data.memo_for,

  // Sender information (omitted for Memorandum for Record)
  ..if data.at("memo_from", default: ()).len() > 0 { (memo_from: data.memo_from) },

  // Subject line
  subject: data.subject,

  // Optional references
  ..if "references" in data { (references: data.references) },

  // Optional footer tag line
  ..if "tag_line" in data { (footer_tag_line: data.tag_line) },

  // Optional classification level
  ..if "classification" in data { (classification_level: data.classification) },

  ..if "dissemination" in data { (dissemination: data.dissemination) },

  // USAF vs DAF memorandum style (date format, body indentation)
  memo_style: data.at("memo_style", default: "usaf"),

  // Font size
  font_size: data.at("font_size", default: 12) * 1pt,

  // List recipients in vertical list
  memo_for_cols: 1,
)

// Mainmatter configuration
#mainmatter[
  #data.at("$body")
]

// Backmatter
#backmatter(
  // Signature block
  signature_block: data.signature_block,
  signing_field: signature-field("Signature"),

  // Optional cc
  ..if "cc" in data { (cc: data.cc) },

  // Optional distribution
  ..if "distribution" in data { (distribution: data.distribution) },

  // Optional attachments
  ..if "attachments" in data { (attachments: data.attachments) },
)

// Indorsements - iterate through CARDS array and filter by CARD tag
#for (i, card) in data.at("$cards").enumerate() {
  if card.at("$kind") == "indorsement" {
    // The quillmark helper leaves an unset/whitespace-only markdown body as
    // the empty string `""`; only non-empty bodies are eval'd into content.
    // Pass truly empty content (`[]`) in the empty case so indorsement can
    // collapse the body's surrounding spacing.
    let body = card.at("$body", default: "")
    let body_content = if type(body) == str { [] } else { body }
    // Per AFH 33-337 Ch. 14, an indorsement is dated when the endorser signs
    // it (distinct from the originating memo's date). Default to today when
    // the card omits or leaves the date blank.
    let card_date = card.at("date", default: none)
    let resolved_date = if card_date == none or card_date == "" {
      datetime.today()
    } else {
      card_date
    }
    indorsement(
      from: card.at("from", default: ""),
      to: card.at("for", default: ""),
      signature_block: card.signature_block,
      signing_field: signature-field("Ind_" + str(i) + "_Signature"),
      ..if "attachments" in card { (attachments: card.attachments) },
      ..if "cc" in card { (cc: card.cc) },
      format: card.at("format", default: "standard"),
      date: resolved_date,
      ..if "action" in card { (action: card.action) },
      body_content,
    )
  }
}
