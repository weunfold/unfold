// Voice ordering, the minimal safe contract: never attach late words to the wrong reply; they fall
// back to a correctly labelled note. Words that arrive after a reply opened never become its
// bubble. No reservation, no reorder.
import test from 'node:test'
import assert from 'node:assert/strict'
import { VoiceLedger } from '../src/voice.mjs'

function fixture() {
  const notes = []
  const ledger = new VoiceLedger({ now: () => 0, onNote: (n) => notes.push(n), timing: { adjacentHoldMs: 1e9 }, epoch: 'fixed' })
  let serial = 0
  const part = (u, seq, text, final = false) => ledger.transcript({ id: `e${++serial}`, kind: 'input_transcript', utterance_id: u, utterance_seq: seq, text, final, association: 'unknown' })
  const take = (r) => ledger.commit({ responseId: r })
  const open = (r, u) => ledger.pin({ responseId: r, ...(u ? { utteranceId: u } : {}) })
  const flush = () => { ledger.closeSegment(); for (const sg of ledger.closed.splice(0)) ledger.showSegment(sg); return notes.map((n) => [n.text, n.final]) }
  return { ledger, notes, part, take, open, flush }
}

// ── safety properties ──

test('explicit identity overrides a binding made before the evidence arrived', () => {
  const f = fixture()
  try {
    f.part('uA', 0, 'belongs to B', true)
    f.open('A')
    f.open('B', 'uA')
    const result = { A: f.take('A'), B: f.take('B') }
    assert.equal(result.A, null, JSON.stringify(result))
    assert.equal(result.B?.text, 'belongs to B')
  } finally { f.ledger.stop() }
})

test('released error-turn words cannot be claimed by the next reply', () => {
  const f = fixture()
  try {
    f.part('uA', 0, 'failed reply input', true)
    f.open('A')
    f.ledger.release('A')
    f.open('B')
    const result = f.take('B')
    assert.equal(result, null, JSON.stringify(result))
  } finally { f.ledger.stop() }
})

test('an unrelated segment cannot discharge the ambiguity of an older missing transcript', () => {
  const f = fixture()
  try {
    f.open('A'); assert.equal(f.take('A'), null)
    f.ledger.voiceStart(); f.open('B')
    f.part('uB', 0, 'new B words arrive first', true)
    assert.equal(f.take('B'), null)
    f.ledger.voiceStart(); f.open('C')
    f.part('uA', 0, 'very late A words', true)
    const result = f.take('C')
    assert.equal(result, null, JSON.stringify(result))
  } finally { f.ledger.stop() }
})

// ── late and partial words under the minimal contract ──

test('a consumed utterance\'s late continuation is never the next reply\'s input', () => {
  const f = fixture()
  try {
    f.part('warm', 0, 'warmup', true)
    f.open('warm-reply'); assert.equal(f.take('warm-reply').text, 'warmup')
    f.open('A'); f.part('uA', 0, 'old partial')
    assert.equal(f.take('A'), null, 'minimal: words after the open are not a bubble')
    f.open('B'); f.part('uA', 1, ' old tail', true)
    assert.equal(f.take('B'), null, 'the safety assertion, unchanged')
    assert.deepEqual(f.flush(), [['old partial', false], ['old tail', true]], 'shown on their own, each with its own finality; nothing lost')
  } finally { f.ledger.stop() }
})

test('a first partial transcript is never reported final without a provider final flag', () => {
  const f = fixture()
  try {
    f.open('A'); f.part('uA', 0, 'first partial', false)
    assert.equal(f.take('A'), null)
    assert.deepEqual(f.flush(), [['first partial', false]])
    // The other path: words heard before the open, still partial, are the bubble and stay partial.
    const h = fixture()
    h.part('uC', 0, 'partial before open', false)
    h.open('C')
    assert.equal(h.take('C').final, false)
    h.ledger.stop()
  } finally { f.ledger.stop() }
})

test('a complete utterance heard before its reply opened is consumed once', () => {
  const f = fixture()
  try {
    f.part('uA', 0, 'good ', false); f.part('uA', 1, 'morning', true)
    f.open('A')
    const t = f.take('A')
    assert.deepEqual([t.text, t.final], ['good morning', true])
    assert.equal(f.take('A'), null)
  } finally { f.ledger.stop() }
})

// ── The minimal contract's own controls ──

test('minimal: OpenAI\'s shape (every reply opens before its words): every utterance is a note, none is attached to any reply', () => {
  const f = fixture()
  try {
    for (const n of [1, 2, 3, 4]) {
      f.ledger.voiceStart()
      f.open(`v${n}`)
      f.part(`u${n}`, 0, `words ${n}`, true)
      assert.equal(f.take(`v${n}`), null)
    }
    assert.deepEqual(f.flush().map(([t]) => t), ['words 1', 'words 2', 'words 3', 'words 4'])
  } finally { f.ledger.stop() }
})

test('minimal: A opens, B opens, A\'s late words: neither reply takes them; one note', () => {
  const f = fixture()
  try {
    f.open('A'); f.open('B')
    f.part('uA', 0, 'late words', true)
    assert.equal(f.take('A'), null)
    assert.equal(f.take('B'), null)
    assert.deepEqual(f.flush(), [['late words', true]])
  } finally { f.ledger.stop() }
})

test('minimal: the Gemini shape (one utterance_id per call, no final flags): the first reply takes its words, later words are notes', () => {
  const f = fixture()
  try {
    f.part('g', 0, 'first words', false); f.open('r1'); assert.equal(f.take('r1').text, 'first words')
    f.ledger.voiceStart()
    f.part('g', 1, 'second words', false); f.open('r2'); assert.equal(f.take('r2'), null)
    assert.deepEqual(f.flush(), [['second words', false]], 'preserved as a note, not final')
  } finally { f.ledger.stop() }
})

test('minimal: explicit ids still pair after a reply opened empty', () => {
  const f = fixture()
  try {
    f.open('A')
    f.ledger.transcript({ id: 'x1', kind: 'input_transcript', text: 'named words', utterance_id: 'u9', utterance_seq: 0, final: true, association: 'provider', response_id: 'B' })
    f.open('B', 'u9')
    assert.equal(f.take('B')?.text, 'named words')
    assert.equal(f.take('A'), null)
  } finally { f.ledger.stop() }
})

// ── partial tails and internal sequence gaps ──
function fixture3() {
  const notes=[]; let serial=0
  const ledger=new VoiceLedger({now:()=>0,onNote:n=>notes.push(n),timing:{adjacentHoldMs:1e9},epoch:'fixed-3'})
  const part=(u,seq,text,final=false)=>ledger.transcript({id:`e${++serial}`,kind:'input_transcript',utterance_id:u,utterance_seq:seq,text,final,association:'unknown'})
  const open=r=>ledger.pin({responseId:r})
  const take=r=>ledger.commit({responseId:r})
  const flush=()=>{ledger.closeSegment();for(const s of ledger.closed.splice(0))ledger.showSegment(s)}
  return {ledger,notes,part,open,take,flush}
}
test('a consumed partial tail before the next open is not new input',()=>{
  const f=fixture3()
  try {
    f.part('uA',0,'old partial',false)
    f.open('A'); assert.equal(f.take('A')?.text,'old partial')
    f.part('uA',1,' old tail',true)
    f.open('B')
    const next=f.take('B')
    assert.equal(next,null,JSON.stringify(next))
    f.flush();assert.ok(f.notes.some(n=>n.text==='old tail'),'tail must be retained as a note')
  } finally {f.ledger.stop()}
})
test('a released partial tail before the next open stays a note',()=>{
  const f=fixture3()
  try {
    f.part('uA',0,'failed partial',false)
    f.open('A'); f.ledger.release('A')
    assert.equal(f.notes[0]?.text,'failed partial')
    f.part('uA',1,' failed tail',true)
    f.open('B')
    const next=f.take('B')
    assert.equal(next,null,JSON.stringify(next))
    f.flush(); assert.ok(f.notes.some(n=>n.text==='failed tail'))
  } finally {f.ledger.stop()}
})
test('an internal sequence gap is not a complete final bubble',()=>{
  const f=fixture3()
  try {
    f.part('uA',0,'first ',false);f.part('uA',2,'third',true)
    f.open('A'); const r=f.take('A')
    assert.equal(r?.final,false,JSON.stringify(r))
    assert.equal(r?.missing,1)
  } finally {f.ledger.stop()}
})
test('an internal sequence gap is not a complete final note',()=>{
  const f=fixture3()
  try {
    f.open('empty');assert.equal(f.take('empty'),null)
    f.part('uA',0,'first ',false);f.part('uA',2,'third',true);f.flush()
    assert.equal(f.notes[0]?.final,false,JSON.stringify(f.notes))
    assert.equal(f.notes[0]?.missing,1)
  } finally {f.ledger.stop()}
})
test('a whole utterance before the open, then sticky notes',()=>{
  const f=fixture3()
  try {
    f.part('uA',0,'whole',true);f.open('A');assert.equal(f.take('A')?.text,'whole')
    f.open('B');assert.equal(f.take('B'),null)
    f.part('uB',0,'late',true);f.open('C');assert.equal(f.take('C'),null)
    assert.equal(f.notes[0]?.text,'late')
  } finally {f.ledger.stop()}
})

// ── fence and gap controls ──

test('a tail that arrives after the reply opened, before it commits, is fenced too', () => {
  const f = fixture3()
  try {
    f.part('uA', 0, 'old partial', false)
    f.open('A')
    f.part('uA', 1, ' old tail', true) // before A's commit
    assert.equal(f.take('A')?.text, 'old partial')
    f.open('B')
    assert.equal(f.take('B'), null)
    f.flush()
    assert.ok(f.notes.some((n) => n.text === 'old tail' && n.final === true))
  } finally { f.ledger.stop() }
})

test('a final segment taken leaves no fence: the next utterance still pairs (OpenAI and Gemini words before a reply)', () => {
  const f = fixture3()
  try {
    f.part('uA', 0, 'whole', true); f.open('A'); assert.equal(f.take('A')?.text, 'whole')
    f.ledger.voiceStart()
    f.part('uB', 0, 'next', true); f.open('B'); assert.equal(f.take('B')?.text, 'next')
  } finally { f.ledger.stop() }
})

test('the fence holds for every backend, the Gemini policy included (no exception)', () => {
  for (const policy of ['provider:gemini-auto-vad@v1', 'provider:openai-realtime-vad@v1', null]) {
    const f = fixture3()
    try {
      f.part('uA', 0, 'first', false); f.ledger.pin({ responseId: 'r1', policy }); assert.equal(f.take('r1')?.text, 'first')
      f.part('uA', 1, 'second', false); f.ledger.pin({ responseId: 'r2', policy }); assert.equal(f.take('r2'), null, String(policy))
      f.flush(); assert.ok(f.notes.some((n) => n.text === 'second' && n.final === false))
    } finally { f.ledger.stop() }
  }
})

test('a gapped bubble shows the gap; a complete one is final with nothing missing', () => {
  const f = fixture3()
  try {
    f.part('uA', 0, 'first ', false); f.part('uA', 2, 'third', true)
    f.open('A')
    const r = f.take('A')
    assert.deepEqual([r.text, r.final, r.missing], ['first [missing part] third', false, 1])
    const g = fixture3()
    g.part('uB', 0, 'one ', false); g.part('uB', 1, 'two', true); g.open('B')
    const q = g.take('B')
    assert.deepEqual([q.text, q.final, q.missing], ['one two', true, 0])
    g.ledger.stop()
  } finally { f.ledger.stop() }
})

// ── the fence under every provider policy ──
for (const policy of ['provider:gemini-auto-vad@v1', 'provider:openai-realtime-vad@v1', null]) {
  test(`consumed partial continuation remains a note under policy ${policy}`,()=>{
    const notes=[]
    const l=new VoiceLedger({now:()=>0,onNote:n=>notes.push(n),timing:{adjacentHoldMs:1e9},epoch:'fixed-4'})
    const part=(seq,text)=>l.transcript({id:`e${seq}`,kind:'input_transcript',utterance_id:'same-user-input',utterance_seq:seq,text,final:false,association:'unknown'})
    try {
      part(0,'old partial')
      l.pin({responseId:'A',policy})
      assert.equal(l.commit({responseId:'A'})?.text,'old partial')
      part(1,' old tail')
      l.pin({responseId:'B',policy})
      const r=l.commit({responseId:'B'})
      assert.equal(r,null,JSON.stringify({policy,misassigned:r,notes}))
      l.closeSegment();for(const s of l.closed.splice(0))l.showSegment(s)
      assert.ok(notes.some(n=>n.text==='old tail'),'tail preserved as a note')
    } finally {l.stop()}
  })
}
