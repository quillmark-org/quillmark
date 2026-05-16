#import "@preview/appreciated-letter:0.1.0": letter

#show: letter.with(
  sender: {{ sender | String }},
  recipient: {{ recipient | String }},
  date: {{ date | String }},
  subject: {{ subject | String }},
  name: {{ name | String }},
)

#{{ body | Content }}