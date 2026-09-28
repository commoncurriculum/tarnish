// ProseMirror's tests make their documents with jsdom's JSDOM; this gives them the same documents
// from linkedom, the DOM tarnish's fixtures are recorded in.
import { DOMParser, parseHTML } from "linkedom"

export class JSDOM {
  constructor(html = "", { contentType = "text/html" } = {}) {
    this.window =
      contentType === "text/html" ? parseHTML(html) : { document: new DOMParser().parseFromString(html, contentType) }
  }
}
