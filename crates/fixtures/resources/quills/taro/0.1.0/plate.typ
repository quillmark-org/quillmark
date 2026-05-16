#import "@local/quillmark-helper:0.1.0": data

#set text(font: "Figtree")

// Advanced: Use show filter to color text
#show regex("(?i)taro"): it => text(fill: purple)[#it]

// Filters like `String` render to code mode automatically,
#underline(data.title)

// When using filters in markup mode,
// add `#` before the template expression to enter code mode.
*Author: #data.author*

*Favorite Ice Cream: #data.ice_cream*__


#data.BODY

// Present each sub-document programatically
#for card in data.CARDS {
  if card.CARD == "quotes" [
    *#card.author*: _#card.BODY _
  ]
}


// Include an image with a dynamic asset
#if "picture" in data {
  image(data.picture)
}
