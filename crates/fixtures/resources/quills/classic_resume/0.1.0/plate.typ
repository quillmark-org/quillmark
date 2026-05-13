#import "@local/quillmark-helper:0.1.0": data
#import "@local/ttq-classic-resume:0.1.0": *

#show: resume

#resume-header(
  name: data.name,
  contacts: data.contacts,
)

#for leaf in data.LEAVES {
  if "title" in leaf and leaf.title != "" {
    section-header(leaf.title)
  }

  if leaf.KIND == "experience_section" {
    timeline-entry(
      heading-left: leaf.at("headingLeft", default: ""),
      heading-right: leaf.at("headingRight", default: ""),
      subheading-left: leaf.at("subheadingLeft", default: none),
      subheading-right: leaf.at("subheadingRight", default: none),
      body: leaf.at("BODY", default: ""),
    )
  } else if leaf.KIND == "skills_section" {
    table(
      columns: 2,
      items: leaf.cells.map(item => (
        category: item.category,
        text: item.skills,
      ))
    )
  } else if leaf.KIND == "projects_section" {
    project-entry(
      name: leaf.name,
      url: leaf.at("url", default: none),
      body: leaf.at("BODY", default: ""),
    )
  } else if leaf.KIND == "certifications_section" {
    table(
      columns: 2,
      items: leaf.cells
    )
  }
}
