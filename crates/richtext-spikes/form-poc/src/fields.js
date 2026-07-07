/** Read a main-card (or any card) field value from the WASM Card wire shape. */
export function fieldValue(card, key) {
  return card?.payloadItems?.find((i) => i.type === "field" && i.key === key)?.value;
}
