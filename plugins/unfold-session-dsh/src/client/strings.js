// What the host says about the Unfold Bot's model menu (src/answer-with.js, src/host-client.js):
// the keys are the English text, as the Bot team's dictionary keeps them (its `omni-ui`
// namespace). The client adds the Chinese with the Bot team's addStrings; the pt-BR pack
// (@automagik/locale-pt-br-dsh) carries the Brazilian Portuguese and checks its keys against
// this file.
export const ANSWER_WITH_ZH = {
  'Unfold is not reachable from here right now': '现在无法从这里连接 Unfold',
  "Unfold's model list could not be read right now": '现在无法读取 Unfold 的模型列表',
  'Unfold does not offer that model or level now': 'Unfold 现在不提供这个模型或级别',
  'Unfold needs your confirmation before it sends your messages there': '需要你确认后，Unfold 才会把你的消息发送到那里',
  'The pick may not have reached Unfold; open the menu again to see': '这次选择可能没有送达 Unfold，请重新打开菜单查看',
}
