#import "@local/quillmark-helper:0.1.0": data

#set page(margin: 1.5in)
#set par(justify: true, leading: 0.75em)

#data.sender

#v(1em)

#data.date

#v(1em)

#data.recipient

#v(2em)

#text(weight: "bold")[#data.subject]

#v(1em)

#data.BODY

#v(2em)

#data.name
