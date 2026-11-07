import assert from 'node:assert/strict'
import { test } from 'node:test'
import { MindStream } from '../../plugins/unfold-mind-dsh/src/mind.js'
import { buildUnfoldTrajectory } from '../../plugins/unfold-trajectory-dsh/src/adapter.mjs'
import { LANE } from '../../plugins/unfold-trajectory-dsh/src/taxonomy.mjs'

function project(rows) {
  const mind = new MindStream({ limit: 1000 })
  let offset = 0
  for (const row of rows) {
    mind.host(row, { via: 'host journal', file: '/fixture/rth-host.jsonl', offset: offset++, length: 1 })
  }
  const snap = mind.snapshot()
  const view = buildUnfoldTrajectory({ rows: snap.rows, episodes: Object.fromEntries(snap.episodes.map(e => [e.id,e])) })
  return view.turns.flatMap(t => t.groups.flatMap(g => g.cells))
}
const fixture = [
  {kind:'started',pid:4242,t_ms:0},
  {kind:'local_vad_onset',t_ms:100,level:-25},
  {kind:'input_audio_end',t_ms:600},
  {kind:'input_transcript',t_ms:640,utterance_id:'mic-1-1',text:'A synthetic request.',final:true},
  {kind:'decision',t_ms:700,decision_id:'floor:1:1',episode_id:'floor:1',trigger_event:'pause',context_rev:1,candidate_schema:'floor.decide/v1',candidates:['WAIT','TAKE_TURN'],selected:'TAKE_TURN',policy:'fixture',latency_ns:60000000,training_eligibility:'not_reviewed'},
  {kind:'playback',t_ms:800,response_id:'clip-1',phase:'started',backchannel:true},
  {kind:'playback',t_ms:1100,response_id:'clip-1',phase:'finished',backchannel:true},
  {kind:'echo_gate',t_ms:1110,action:'hold_clock'},
  {kind:'playback',t_ms:1200,response_id:'reply-1',phase:'started'},
  {kind:'playback',t_ms:1500,response_id:'reply-1',phase:'cleared'},
  {kind:'speaker_cleared',t_ms:1500,response_id:'reply-1'},
]
test('floor host journal reaches existing Hear, Speak and Act lanes with honest clocks', () => {
  const cells=project(fixture)
  assert(cells.some(c=>c.unfoldKind==='vad' && c.unfoldLane===LANE.HEAR))
  assert(cells.some(c=>c.unfoldKind==='voice' && c.unfoldLane===LANE.HEAR))
  assert.equal(cells.filter(c=>c.unfoldKind==='decision' && c.unfoldLane===LANE.ACT).length,1)
  assert(cells.some(c=>c.unfoldKind==='speaking' && c.unfoldLane===LANE.SPEAK && c.unfoldSpan==='measured' && c.unfoldTiming[0].ms===300))
  assert(cells.some(c=>c.unfoldKind==='interrupt'))
  assert(cells.some(c=>c.unfoldKind==='echo'))
  // This adapter currently renders Hear and Act as ticks, not measured spans.
  assert(cells.filter(c=>['vad','decision'].includes(c.unfoldKind)).every(c=>c.unfoldSpan==='instant'))
})
test('missing or cross-lifetime playback endpoint cannot manufacture a Speak span', () => {
  const open=project(fixture.filter(r=>!(r.kind==='playback' && r.phase==='finished')))
  assert(open.filter(c=>c.unfoldKind==='speaking' && c.unfoldRows.length===1).every(c=>c.unfoldSpan!=='measured'))
  const cross=project([{kind:'started',pid:1,t_ms:0},{kind:'playback',response_id:'same',phase:'started',t_ms:100},{kind:'started',pid:2,t_ms:0},{kind:'playback',response_id:'same',phase:'finished',t_ms:200}])
  assert(cross.filter(c=>c.unfoldKind==='speaking').every(c=>c.unfoldSpan!=='measured'))
})

test('interim decision rows remain on the trajectory decision lane', () => {
  const cells = project([
    {kind:'started',pid:4242,t_ms:0},
    {kind:'decision',event_type:'decision_made',source:'interim',t_ms:300,
     decision_id:'interim:1:1:1',episode_id:'floor:1:1',trigger_event:'post_aec_voice_or_asr',
     context_rev:1,candidate_schema:'floor.interim/v1',candidates:['HOLD','RESUME','YIELD'],
     selected:'HOLD',policy:'audio-duration-word-count',latency_ns:0,training_eligibility:'not_reviewed'},
  ])
  assert.equal(cells.filter(c => c.unfoldKind === 'decision').length, 1)
})

test('Smart Turn decisions preserve probability and reach the Act lane', () => {
  const cells=project([{kind:'started',pid:4242,t_ms:0},{kind:'decision',event_type:'decision_made',source:'smart-turn',part:'turn.detect',t_ms:500,decision_id:'smart-turn:1:1:500',episode_id:'floor:1:1',trigger_event:'post_aec_silero_silence',context_rev:1,candidate_schema:'turn.detect/v1',candidates:['WAIT','TAKE_TURN'],selected:'WAIT',probability:0.12,S_ms:256,T:0.5,max_hold_ms:2400,audio_start_ms:0,audio_end_ms:500,latency_ns:65000000,training_eligibility:'not_reviewed'}]);
  const decision=cells.find(c=>c.unfoldKind==='decision' && c.unfoldLane===LANE.ACT);
  assert(decision);
  const mind=new MindStream({limit:100});
  mind.host({kind:'decision',source:'smart-turn',probability:0.12,t_ms:500},{via:'host journal',file:'/fixture/rth-host.jsonl',offset:0,length:1});
  assert.equal(mind.rows[0].fields.probability,0.12);
  assert.equal(mind.rows[0].fields.source,'smart-turn');
})
