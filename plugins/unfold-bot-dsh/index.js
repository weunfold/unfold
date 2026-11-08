// Unfold for DeepSeek Harness, on the Bot team (@automagik/bots-dsh): the Unfold Bot, Unfold
// inside DSH. Its default plugins (the Unfold bridge and the Unfold plugins) are installed beside
// it; this plugin makes the Bot and names them.
//
// The team's runtime arrives as the omniTeam service. This plugin's runtime has the team's as
// its prototype, so what it adds sits on this plugin's fiber and goes when the plugin does.
import * as unfold from './src/host/unfold.js'

export const name = 'unfold-bot'
// Without the team this plugin waits (DSH shows it pending), as Genie's does.
export const inject = ['omniTeam']

export function apply(ctx, config = {}) {
  if (unfold.unfoldLeads()) {
    // The team waits for its host-owned root; an early browser read makes no default Bot.
    ctx.inject(['unfoldSession'], (sctx) => { createRuntime(sctx, ctx.omniTeam, config) })
  } else createRuntime(ctx, ctx.omniTeam, config)
}

export function createRuntime(ctx, team, config = {}, env = process.env) {
  const rt = Object.assign(Object.create(team), { ctx, unfoldConfig: config })
  unfold.install(rt, team, env)
  return rt
}
