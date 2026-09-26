// prosemirror-transform's API over tarnish. As in prosemirror-model.mjs, the classes pass calls
// through to the native bridge with the handles of their arguments; what's here is JavaScript's
// side of the boundary.
import { Fragment, Slice } from "prosemirror-model"
import native from "./native.mjs"

export let TransformError = function TransformError(message) {
  let err = Error.call(this, message)
  err.__proto__ = TransformError.prototype
  return err
}
TransformError.prototype = Object.create(Error.prototype)
TransformError.prototype.constructor = TransformError
TransformError.prototype.name = "TransformError"

function handles(wrappers) {
  return wrappers.map(wrapper => wrapper.h)
}

// `{type, attrs}` wrappers with the handles of their types, `null` for an empty entry.
function wrapperArgs(wrappers) {
  return wrappers.map(wrapper => wrapper ? { type: wrapper.type.h, attrs: wrapper.attrs } : null)
}

export class MapResult {
  /// @internal
  constructor(pos, delInfo, recover, deleted, deletedBefore, deletedAfter, deletedAcross) {
    this.pos = pos
    this.delInfo = delInfo
    this.recover = recover
    this.deleted = deleted
    this.deletedBefore = deletedBefore
    this.deletedAfter = deletedAfter
    this.deletedAcross = deletedAcross
  }
}

const HANDLE = Symbol("handle")

export class StepMap {
  constructor(ranges, inverted = false) {
    if (ranges === HANDLE) {
      this.h = inverted
      return
    }
    if (!ranges.length && StepMap.empty) return StepMap.empty
    this.h = new native.StepMapHandle(ranges, inverted)
  }

  get ranges() { return this.h.ranges }
  get inverted() { return this.h.inverted }
  /// @internal
  recover(value) { return this.h.recover(value) }
  mapResult(pos, assoc = 1) { return this.h.mapResult(pos, assoc) }
  map(pos, assoc = 1) { return this.h.map(pos, assoc) }
  /// @internal
  touches(pos, recover) { return this.h.touches(pos, recover) }
  forEach(f) { this.h.forEach(f) }
  invert() { return this.h.invert() }
  toString() { return this.h.toDebugString() }
  static offset(n) { return native.stepMapOffset(n) }
}

StepMap.empty = new StepMap([])

export class Mapping {
  constructor(maps, mirror, from = 0, to = maps ? maps.length : 0) {
    if (maps === HANDLE) {
      this.h = mirror
      return
    }
    this.h = new native.MappingHandle(maps ? handles(maps) : [], mirror, from, to)
  }

  get maps() { return this.h.maps() }
  /// @internal
  get mirror() { return this.h.mirror }
  get from() { return this.h.from }
  get to() { return this.h.to }
  slice(from = 0, to = this.maps.length) { return this.h.slice(from, to) }
  appendMap(map, mirrors) { this.h.appendMap(map.h, mirrors) }
  appendMapping(mapping) { this.h.appendMapping(mapping.h) }
  getMirror(n) { return this.h.getMirror(n) ?? undefined }
  /// @internal
  setMirror(n, m) { this.h.setMirror(n, m) }
  appendMappingInverted(mapping) { this.h.appendMappingInverted(mapping.h) }
  invert() { return this.h.invert() }
  map(pos, assoc = 1) { return this.h.map(pos, assoc) }
  mapResult(pos, assoc = 1) { return this.h.mapResult(pos, assoc) }
}

const stepsByID = Object.create(null)

export class Step {
  #h

  /// @internal
  get h() { return (this.#h ??= this.handle()) }
  getMap() { return this.h.getMap() }
  apply(doc) { return this.h.apply(doc.h) }
  invert(doc) { return this.h.invert(doc.h) }
  map(mapping) { return this.h.map(mapping.h) }
  merge(other) { return this.h.merge(other.h) }
  toJSON() { return this.h.toJson() }

  static fromJSON(schema, json) { return native.stepFromJson(schema.h, json) }

  static jsonID(id, stepClass) {
    if (id in stepsByID) throw new RangeError("Duplicate use of step JSON ID " + id)
    if (!builtIn.has(id)) throw new RangeError("tarnish applies only ProseMirror's own steps, not " + id)
    stepsByID[id] = stepClass
    stepClass.prototype.jsonID = id
    return stepClass
  }
}

const builtIn = new Set(["replace", "replaceAround", "addMark", "removeMark", "addNodeMark", "removeNodeMark", "attr", "docAttr"])

export class StepResult {
  /// @internal
  constructor(doc, failed) {
    this.doc = doc
    this.failed = failed
  }

  static ok(doc) { return new StepResult(doc, null) }
  static fail(message) { return new StepResult(null, message) }
  static fromReplace(doc, from, to, slice) { return native.stepResultFromReplace(doc.h, from, to, slice.h) }
}

export class ReplaceStep extends Step {
  constructor(from, to, slice, structure = false) {
    super()
    this.from = from
    this.to = to
    this.slice = slice
    this.structure = structure
  }

  /// @internal
  handle() { return native.StepHandle.replace(this.from, this.to, this.slice.h, !!this.structure) }
  static fromJSON(schema, json) { return Step.fromJSON(schema, { ...json, stepType: "replace" }) }
}

export class ReplaceAroundStep extends Step {
  constructor(from, to, gapFrom, gapTo, slice, insert, structure = false) {
    super()
    this.from = from
    this.to = to
    this.gapFrom = gapFrom
    this.gapTo = gapTo
    this.slice = slice
    this.insert = insert
    this.structure = structure
  }

  /// @internal
  handle() {
    return native.StepHandle.replaceAround(this.from, this.to, this.gapFrom, this.gapTo, this.slice.h, this.insert, !!this.structure)
  }

  static fromJSON(schema, json) { return Step.fromJSON(schema, { ...json, stepType: "replaceAround" }) }
}

export class AddMarkStep extends Step {
  constructor(from, to, mark) {
    super()
    this.from = from
    this.to = to
    this.mark = mark
  }

  /// @internal
  handle() { return native.StepHandle.addMark(this.from, this.to, this.mark.h) }
  static fromJSON(schema, json) { return Step.fromJSON(schema, { ...json, stepType: "addMark" }) }
}

export class RemoveMarkStep extends Step {
  constructor(from, to, mark) {
    super()
    this.from = from
    this.to = to
    this.mark = mark
  }

  /// @internal
  handle() { return native.StepHandle.removeMark(this.from, this.to, this.mark.h) }
  static fromJSON(schema, json) { return Step.fromJSON(schema, { ...json, stepType: "removeMark" }) }
}

export class AddNodeMarkStep extends Step {
  constructor(pos, mark) {
    super()
    this.pos = pos
    this.mark = mark
  }

  /// @internal
  handle() { return native.StepHandle.addNodeMark(this.pos, this.mark.h) }
  static fromJSON(schema, json) { return Step.fromJSON(schema, { ...json, stepType: "addNodeMark" }) }
}

export class RemoveNodeMarkStep extends Step {
  constructor(pos, mark) {
    super()
    this.pos = pos
    this.mark = mark
  }

  /// @internal
  handle() { return native.StepHandle.removeNodeMark(this.pos, this.mark.h) }
  static fromJSON(schema, json) { return Step.fromJSON(schema, { ...json, stepType: "removeNodeMark" }) }
}

export class AttrStep extends Step {
  constructor(pos, attr, value) {
    super()
    this.pos = pos
    this.attr = attr
    this.value = value
  }

  /// @internal
  handle() { return native.StepHandle.attr(this.pos, this.attr, this.value) }
  static fromJSON(schema, json) { return Step.fromJSON(schema, { ...json, stepType: "attr" }) }
}

export class DocAttrStep extends Step {
  constructor(attr, value) {
    super()
    this.attr = attr
    this.value = value
  }

  /// @internal
  handle() { return native.StepHandle.docAttr(this.attr, this.value) }
  static fromJSON(schema, json) { return Step.fromJSON(schema, { ...json, stepType: "docAttr" }) }
}

Step.jsonID("replace", ReplaceStep)
Step.jsonID("replaceAround", ReplaceAroundStep)
Step.jsonID("addMark", AddMarkStep)
Step.jsonID("removeMark", RemoveMarkStep)
Step.jsonID("addNodeMark", AddNodeMarkStep)
Step.jsonID("removeNodeMark", RemoveNodeMarkStep)
Step.jsonID("attr", AttrStep)
Step.jsonID("docAttr", DocAttrStep)

export class Transform {
  #steps = []
  #docs = []
  #mapping

  constructor(doc) {
    this.h = new native.TransformHandle(doc.h)
  }

  // The steps and documents the native transform added since they were last read.
  #sync() {
    if (this.#steps.length < this.h.stepCount) {
      this.#steps.push(...this.h.stepsFrom(this.#steps.length))
      this.#docs.push(...this.h.docsFrom(this.#docs.length))
    }
  }

  get doc() { return this.h.doc() }
  get steps() { this.#sync(); return this.#steps }
  get docs() { this.#sync(); return this.#docs }
  get mapping() { return (this.#mapping ??= this.h.mapping()) }
  get before() { return this.h.before() }
  get docChanged() { return this.h.docChanged }

  step(step) {
    this.#sync()
    this.h.step(step.h)
    this.#steps.push(step)
    this.#docs.push(...this.h.docsFrom(this.#docs.length))
    return this
  }

  maybeStep(step) {
    this.#sync()
    let result = this.h.maybeStep(step.h)
    if (!result.failed) {
      this.#steps.push(step)
      this.#docs.push(...this.h.docsFrom(this.#docs.length))
    }
    return result
  }

  changedRange() { return this.h.changedRange() }
  replace(from, to = from, slice = Slice.empty) { this.h.replace(from, to, slice.h); return this }
  replaceWith(from, to, content) { this.h.replaceWith(from, to, Fragment.from(content).h); return this }
  delete(from, to) { this.h.delete(from, to); return this }
  insert(pos, content) { this.h.insert(pos, Fragment.from(content).h); return this }
  replaceRange(from, to, slice) { this.h.replaceRange(from, to, slice.h); return this }
  replaceRangeWith(from, to, node) { this.h.replaceRangeWith(from, to, node.h); return this }
  deleteRange(from, to) { this.h.deleteRange(from, to); return this }
  lift(range, target) { this.h.lift(range.h, target); return this }
  join(pos, depth = 1) { this.h.join(pos, depth); return this }
  wrap(range, wrappers) { this.h.wrap(range.h, wrapperArgs(wrappers)); return this }
  setBlockType(from, to = from, type, attrs = null) { this.h.setBlockType(from, to, type.h, attrs); return this }

  setNodeMarkup(pos, type, attrs = null, marks) {
    this.h.setNodeMarkup(pos, type ? type.h : null, attrs, marks ? handles(marks) : null)
    return this
  }

  setNodeAttribute(pos, attr, value) { this.h.setNodeAttribute(pos, attr, value); return this }
  setDocAttribute(attr, value) { this.h.setDocAttribute(attr, value); return this }
  addNodeMark(pos, mark) { this.h.addNodeMark(pos, mark.h); return this }
  removeNodeMark(pos, mark) { this.h.removeNodeMark(pos, mark.h); return this }
  split(pos, depth = 1, typesAfter) { this.h.split(pos, depth, typesAfter ? wrapperArgs(typesAfter) : null); return this }
  addMark(from, to, mark) { this.h.addMark(from, to, mark.h); return this }
  removeMark(from, to, mark) { this.h.removeMark(from, to, mark?.h); return this }
  clearIncompatible(pos, parentType, match) { this.h.clearIncompatible(pos, parentType.h, match ? match.h : null); return this }
}

export function replaceStep(doc, from, to = from, slice = Slice.empty) {
  return native.replaceStep(doc.h, from, to, slice.h)
}

export function canJoin(doc, pos) { return native.canJoin(doc.h, pos) }

export function canSplit(doc, pos, depth = 1, typesAfter) {
  return native.canSplit(doc.h, pos, depth, typesAfter ? wrapperArgs(typesAfter) : null)
}

export function joinPoint(doc, pos, dir = -1) { return native.joinPoint(doc.h, pos, dir) ?? undefined }
export function insertPoint(doc, pos, nodeType) { return native.insertPoint(doc.h, pos, nodeType.h) }
export function dropPoint(doc, pos, slice) { return native.dropPoint(doc.h, pos, slice.h) }
export function liftTarget(range) { return native.liftTarget(range.h) }

export function findWrapping(range, nodeType, attrs = null, innerRange = range) {
  return native.findWrapping(range.h, nodeType.h, attrs, innerRange.h)
}

native.register({
  makeStep: (id, ...args) => new stepsByID[id](...args),
  makeStepResult: (doc, failed) => new StepResult(doc, failed),
  wrapStepMap: h => (h.ranges.length ? new StepMap(HANDLE, h) : StepMap.empty),
  wrapMapping: h => new Mapping(HANDLE, h),
  wrapMapResult: (...args) => new MapResult(...args),
  TransformError,
})
