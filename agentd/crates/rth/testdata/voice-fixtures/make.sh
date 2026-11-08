#!/bin/sh
# Generate local voice-test audio using the named macOS voices and ffmpeg.
# The generated WAVs stay local; verify them against the pinned manifest.
set -eu
D=$(cd "$(dirname "$0")" && pwd)
cd "$D"
s() { # voice out text
  say -v "$1" -o "$2.tmp.wav" --data-format=LEI16@16000 "$3"
  if [ "${4:-}" = phone ]; then
    ffmpeg -loglevel error -y -i "$2.tmp.wav" -af "highpass=f=300,lowpass=f=3400,volume=-6dB" -ar 16000 -ac 1 -c:a pcm_s16le "$2.wav"
    rm "$2.tmp.wav"
  else
    mv "$2.tmp.wav" "$2.wav"
  fi
}
U="Reed (Portuguese (Brazil))"   # the user (near end)
P="Flo (Portuguese (Brazil))"    # the other person, on a phone on speaker
A="Luciana"                      # stands in for the agent's own voice (far end)
# far end: a long agent reply
s "$A" agent-reply "Para implementar o EDI, você vai precisar de padrões de dados definidos, protocolos de comunicação seguros, e um software de tradução que converta os seus documentos para o formato padrão. Depois disso, é só testar com um parceiro e colocar em produção."
# near-end interruptions
s "$U" barge-short "Espera, para."
s "$U" barge-long "Não, não é isso. Me explica de novo a segunda parte."
s "$U" barge-word "Para."
# Side conversation phrases
s "$U" s01-user-to-other "Então, amanhã a gente fecha aquele contrato com o cliente, beleza?"
s "$P" s02-phone-to-user "Beleza, eu mando a proposta revisada até o meio dia." phone
s "$U" s03-user-to-rth "RTH, quanto é doze vezes doze?"
s "$U" s04-user-to-other "Desculpa, eu tava falando com o assistente. Onde a gente parou?"
s "$P" s05-phone-to-user "Na parte do prazo de entrega, que ficou para sexta." phone
s "$U" s06-user-to-rth "RTH, me explica em uma frase o que é EDI."
s "$P" s07-phone-during-reply "Alô? Você ainda tá aí? Eu não tô te ouvindo direito." phone
s "$U" s08-user-to-other "Tô sim, só um minuto que eu já falo contigo."
s "$U" s09-user-to-rth-english "RTH, responde em inglês, por favor: qual é a capital da França?"
s "$U" s10-user-to-rth-portuguese "RTH, agora volta para o português: qual é a capital da Itália?"
s "$U" s11-user-to-rth-long "RTH, conta devagar de um até quinze."
s "$U" s12-user-tail-interrupt "RTH, para. Já entendi."
# leading silence for the scripted microphone
ffmpeg -loglevel error -y -f lavfi -i anullsrc=r=16000:cl=mono -t 1 -c:a pcm_s16le silence-1s.wav
shasum -a 256 -c SHA256SUMS
