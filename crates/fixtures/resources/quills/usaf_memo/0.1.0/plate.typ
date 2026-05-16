#import "@local/quillmark-helper:0.1.0": data
#import "@local/tonguetoquill-usaf-memo:1.0.0": backmatter, frontmatter, indorsement, mainmatter

// Frontmatter configuration
#show: frontmatter.with(
  // Letterhead configuration
  letterhead_title: data.letterhead_title,
  letterhead_caption: data.letterhead_caption,
  letterhead_seal: image("assets/dow_seal.jpg"),

  // Date
  date: data.date,

  // Receiver information
  memo_for: data.memo_for,

  // Sender information
  memo_from: data.memo_from,

  // Subject line
  subject: data.subject,

  // Optional references
  ..if "references" in data { (references: data.references) },

  // Optional footer tag line
  ..if "tag_line" in data { (footer_tag_line: data.tag_line) },

  // Optional classification level
  ..if "classification" in data { (classification_level: data.classification) },

  // Font size
  ..if "font_size" in data { (font_size: float(data.font_size) * 1pt) },

  // List recipients in vertical list
  memo_for_cols: 1,
)

// Mainmatter configuration
#mainmatter[
  #data.BODY
]

// Backmatter
#backmatter(
  // Signature block
  signature_block: data.signature_block,

  // Optional cc
  ..if "cc" in data { (cc: data.cc) },

  // Optional distribution
  ..if "distribution" in data { (distribution: data.distribution) },

  // Optional attachments
  ..if "attachments" in data { (attachments: data.attachments) },
)

// Indorsements - iterate through CARDS array and filter by KIND type
#for card in data.CARDS {
  if card.KIND == "indorsement" {
    indorsement(
      from: card.at("from", default: ""),
      to: card.at("for", default: ""),
      signature_block: card.signature_block,
      ..if "attachments" in card { (attachments: card.attachments) },
      ..if "cc" in card { (cc: card.cc) },
      format: card.at("format", default: "standard"),
      ..if "date" in card { (date: card.date) },
    )[
      #card.BODY
    ]
  }
}
