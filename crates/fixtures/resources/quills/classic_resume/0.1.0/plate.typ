#import "@local/quillmark-helper:0.1.0": data
#import "@local/ttq-classic-resume:0.1.0": *

#show: resume

#resume-header(
  name: data.name,
  contacts: data.contacts,
)

#for card in data.CARDS {
  if "title" in card and card.title != "" {
    section-header(card.title)
  }

  if card.CARD == "experience_section" {
    timeline-entry(
      heading-left: card.at("heading_left", default: ""),
      heading-right: card.at("heading_right", default: ""),
      subheading-left: card.at("subheading_left", default: none),
      subheading-right: card.at("subheading_right", default: none),
      body: card.at("BODY", default: ""),
    )
  } else if card.CARD == "skills_section" {
    table(
      columns: 2,
      items: card.cells.map(item => (
        category: item.category,
        text: item.skills,
      ))
    )
  } else if card.CARD == "projects_section" {
    project-entry(
      name: card.name,
      url: card.at("url", default: none),
      body: card.at("BODY", default: ""),
    )
  } else if card.CARD == "certifications_section" {
    table(
      columns: 2,
      items: card.cells
    )
  }
}
