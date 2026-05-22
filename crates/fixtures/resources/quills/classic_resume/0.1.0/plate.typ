#import "@local/quillmark-helper:0.1.0": data
#import "@local/ttq-classic-resume:0.1.0": *

#show: resume

#resume-header(
  name: data.name,
  contacts: data.contacts,
)

#for card in data.at("$cards") {
  if "title" in card and card.title != "" {
    section-header(card.title)
  }

  let kind = card.at("$kind")
  if kind == "experience_section" {
    timeline-entry(
      heading-left: card.at("heading_left", default: ""),
      heading-right: card.at("heading_right", default: ""),
      subheading-left: card.at("subheading_left", default: none),
      subheading-right: card.at("subheading_right", default: none),
      body: card.at("$body", default: ""),
    )
  } else if kind == "skills_section" {
    table(
      columns: 2,
      items: card.cells.map(item => (
        category: item.category,
        text: item.skills,
      ))
    )
  } else if kind == "projects_section" {
    project-entry(
      name: card.name,
      url: card.at("url", default: none),
      body: card.at("$body", default: ""),
    )
  } else if kind == "certifications_section" {
    table(
      columns: 2,
      items: card.cells
    )
  }
}
