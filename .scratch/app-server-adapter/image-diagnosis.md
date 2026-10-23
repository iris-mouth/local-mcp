# MCP画像のモデル視覚利用の切り分け

Date: 2026-10-23 (Asia/Tokyo)
Status: content_items image output and blind visual recognition passed in ordinary Chat; oracle verified

## 結果

受入記録は`38046ba`でコミットした。今回の直接添付対照試験では、前回MCP経由で認識できなかった同じPNGを、同じ通常Chatが正しく認識した。画像自体とそのChatの視覚機能の一般的な失敗では説明できない。新実装固有のregressionとも断定できない。

新規通常Chatの構造検査では、返却オブジェクトが`{content: string, content_items: ...}`だった。Chatが想定した標準MCPの`content: ContentBlock[]`ではなく、画像出力の条件分岐に入っていなかった。この呼出しで画像がモデルへ出力されない直接の要因は返却構造の取り違え。後続試験で`content_items[0]`にnative imageが保持されていることと、image helperへ渡すことで通常Chatが5図形の視覚回答を生成できることを確認した。本人許可後の正解manifestとの事後照合で、5図形すべての色・形・相対位置が一致した。画像bytes自体の恒常的なTunnel欠落とは区別する。

## 判定点と手順上の制約

判定点は未見の画像について図形数・色・形・位置を答えられること。MCP image blockの存在、HTTP成功、UIの解析中表示は代替の合格条件にしない。実際のモデル視覚経路は通常Chat UIにあり、ローカルの高速な単体テストだけでは到達できない。このためdiagnosing-bugsの「秒単位・単一CLI」の条件を満たしたとは主張せず、CUAによる実UI操作と盲検fixtureの対照試験を用いる。原因がhost内部なら、このrepoには適切な回帰テストの接合点がまだない。

検証前に提示した仮説と予測:

1. Chat全体の視覚機能または画像自体が原因なら、同じPNGの直接添付も失敗する。
2. 既存会話の状態が原因なら、新規ChatのMCP経由は成功する。
3. Tunnel以降のMCP画像受渡しが原因なら、直接添付は成功しMCPだけ失敗する。
4. 新応答構造が原因なら、旧image-only応答は成功する（前回の旧版失敗は反証材料）。

cloud境界で画像保持を確認した後、次の追加仮説を検証前に提示した: ツール実行用JavaScriptが受け取ったimage blockをモデルへ明示的に出力していないなら、MCP再取得なしで取得済みblockをimageとして出力するだけで視認できる。[Programmatic Tool Calling公式資料](https://developers.openai.com/api/docs/guides/tools-programmatic-tool-calling)はtext/imageの出力を区別し、中間結果がすべてモデルcontextへ入るわけではないことを説明する。通常Chatの失敗時に実際にこの呼出し方をしたかは未確認であり、資料から断定しない。

## 観測

| 経路 | 観測 | 証明できない点 |
|---|---|---|
| 新view_image → ローカルstdio wrapper | text,image。PNG 4329 bytes、fixtureと完全一致（前回受入） | Tunnel以降の保持、モデル視覚 |
| 旧get_image → ローカルstdio wrapper | imageのみ。同じbytes（今回も再確認） | Tunnel以降の保持、モデル視覚 |
| 旧get_image → 既存Tunnel → 接続元codex_appsツール | image/png native block、base64 5772 chars。元fixtureと完全一致（今回） | 通常Chatのtool-result→model変換 |
| 新view_image → 既存Tunnel → 通常Chat | メタデータは取得、図柄未認識（前回受入） | cloud内部で消失した具体的な層 |
| 旧get_image → 既存Tunnel → 同じ通常Chat | 例外報告なし、図柄未認識（前回受入） | 画像のcloud到達そのもの |
| 同じPNGを同じChatへ直接添付 | 5図形の位置・色・形すべて正解（今回） | 別経路のMCP画像の処理 |

fixture SHA-256: `d703559f29c241380de9e42969c36d398fff85f35c35e6a4785145d0c10df691`。

直接添付の回答を得た後で初めて正解manifestを読み、上段左=緑円、上段中央=赤円、上段右=黄正方形、下段左=青三角形、下段右=紫正方形との完全一致を確認した。正解manifestはChatへ提示していない。この画像は以後同じChatでは未見でないため、新規のMCP盲検には別fixtureが必要。

通常Chat: https://chatgpt.com/c/6adb1cea-a160-83ec-ae60-82ea97f56eb0

今回のローカル境界再検査:

```text
python3 /private/tmp/local-mcp-chat-acceptance-sxjd1vm3/probe-image-stdio.py
content_types=[image], mime_type=image/png, bytes=4329,
exact_fixture_match=true, model_vision_proven=false
```

このcommandはローカル境界用であり、モデル視覚失敗を検出できる回帰テストとは扱わない。

## PR・fork調査

通常sandbox内では`gh`がapi.github.comへ接続失敗し、sandbox外の同じ公開読取は成功した。接続失敗をGitHub障害とは扱わない。

- 上流PRは全stateで8件（#1,#2,#3,#5,#6,#7,#8,#9）を取得。画像配送の修正はなかった。#6は認証付きHTTP transport、#7は別CLIへの委譲であり、今回それらの接続・推論基盤を導入しない。
- 公開fork一覧35件を取得し、2ページ目が空であることを確認。全forkの既定ブランチを上流mainとcompareし、aheadはkota113、TPYAHOYAHA、jsaaa、nenekaの4件。他はidenticalまたはbehind。既定ブランチ外の全branch・削除forkまで調査した保証はない。
- [jsaaa/local-mcp](https://github.com/jsaaa/local-mcp/commit/3d6c052fc8093940a47b52d3bab2015b5eb97cd7)の36追加commitの集約diffを確認。画像返却部分の変更はなく、主な追加は出力・job・workflow等。今回の画像問題の既知修正は見つからなかった。
- [別プロジェクトのPR #53](https://github.com/buonchoi96/codex-chatgpt-web/pull/53)は非同期pollでnative imageをJSON文字列へ潰す欠陥を修正する。今回のlocal-mcp画像は直接応答で、そのbroker/poll実装は使っていない。構造を保つ検査の参考にはなるが、移植すれば直る根拠にはならない。
- [公式Tunnel issue #91](https://github.com/openai/tunnel-client/issues/91)では完全な応答をControl Planeへ配送後、ChatGPT結果が{}になる別の障害が報告されている。nested contentというJSON field名の変更で回避した報告と、下流ChatGPT処理を示唆する保守者のコメントがある。今回のimage-only応答にはそのnested text JSON objectがないため、同一原因と扱わない。
- [aspicio issue #65](https://github.com/frontsail-ai/aspicio/issues/65)はモデルが画像を認識できるがユーザーUIに表示されないという報告。今回のモデル未認識とは異なるため、UIに画像が見えないことだけで同一障害としない。

## 公式資料と現在のclient

既存clientは`0.0.15+a390c168ff1b2d14e73a95991c186c6aba3ff5a0`。同revisionの公開ソースではdispatcherがJSON-RPC responseをEncodeMessageし、control-plane clientがraw JSON responseをresp_jsonへ入れる。ソース上の一般的な透過処理だけでは、実リクエストの画像保持を証明しない。

[Secure MCP Tunnel仕様](https://developers.openai.com/api/docs/guides/secure-mcp-tunnels)はserialized envelopeの上限を10 MiBとする。今回のPNGは約4 KiBであり、単純な容量超過説を支持しない。新旧とも4329 bytesで、新実装のstructuredContent有無だけでは旧版失敗を説明できない。

[plugin tool result仕様](https://developers.openai.com/plugins/reference)はcontentとstructuredContentのmodelへの公開を説明するが、今回の通常Chat/Tunnelにおける画像処理の具体的な保証・内部故障点は確認できなかった。

[v0.0.17 release](https://github.com/openai/tunnel-client/releases/tag/v0.0.17)はstartup recoveryと任意診断を主に説明しており、画像欠落の修正を明示しない。版が新しいだけで解決と推測せず、更新はしていない。

## 追加試験の状態

共有catalogを変更せず旧10ツールの既存起動scriptを同じ使い捨てprojectで一時起動した。sessionは`1bd6b1f8-6bda-4e01-b633-be103bad7be4`。本人のKeychain一度だけ許可後、readyz=200。接続元ツールでsession_infoのcwd一致とget_image応答のnative image保持を確認した。base64文字列を元fixtureのencodeと比較し完全一致。接続元モデルへ明示的にimage blockを提示すると図形を視認できた。通常Chatの視覚成功とは区別する。

この結果は、当該呼出しでローカルwrapper・既存Tunnel・接続元ツールまで画像が欠落しなかった実測である。通常Chatの呼出しも同一内部処理を通る保証はないが、共通Tunnelで常に画像が落ちるという仮説は反証された。残る主対象は通常Chatのtool-resultから視覚入力への処理、および会話状態の影響。

次の未見fixtureとしてvision-probe2.pngを生成し、正解はまだ読んでいない。新規通常Chatへ既存local-workspaceを選択したが、試験用Session IDとcwdを含む入力は自動承認レビューに拒否された。理由は新規Chatという宛先へ識別子・内部パスを送る具体的許可が未確認であること。別経路で送らず、本人へ対象・用途・試験後終了を明記して許可を依頼した。プロンプト送信前であり、この新規ChatでのMCP呼出しはまだ未実施。

次の判別点は、未見fixtureを新規Chatで旧get_imageから取得できるか、および同じTunnelを通った接続元ツール応答でimage blockが保持されるか。実際のcloud入口・出口を観測しないままTunnel clientとChatGPT内部のどちらかへ断定しない。raw HTTP全量log、認証情報取得、provider変更、公開listener追加は行わない。

## 新規Chat試験の再開

本人が上記IDと作業先の新規Chatへの送信を明示許可した後、同じプロンプトを送信した。

Chat: https://chatgpt.com/c/6adb2dc8-ce7c-83ee-aa70-25fa5726a53d

- session_infoは1回成功しcwd一致。
- get_image(vision-probe2.png)を1回実施。Chatは「画像本体未確認」「空のオブジェクト{}が複数」と報告した。新規Chatでも失敗し、以前の会話だけに起因する仮説を支持しない。
- MCP再取得を禁じたまま、取得済みimage blockの明示的出力だけを依頼。Chatは初回からimage(c)を用意したと報告したが、トップレベルキーとcontent各typeは未記録で、既に消えたローカル変数rへアクセスできなかった。可視出力は{}が32個だったという。これはChatの実行に関する自己報告であり、元オブジェクトの型を直接観測した証拠ではない。
- 同じfixtureを接続元から追加取得する対照呼出しは、自動承認レビューにより「再呼出し禁止の試験手順に抵触」として拒否。返却されたtext/isErrorは承認拒否通知であり、MCPサーバーの画像応答として集計しない。迂回せず、返却構造を記録する追加1回の例外許可を本人へ依頼した。
- 表示履歴の旧要素番号でのclickもレビュー拒否。最新treeで18s考えましたボタンを照合して開くと概要だけが表示された。再生成やMCP再実行はしていない。

返却構造が直接観測されるまで、通常Chat側の正規化でimage blockが消えたのか、ツール実行用JavaScriptが別の返却型を想定していたのかを区別できない。

## 許可された追加1回の構造検査

本人の「追加1回の画像取得を許可する」後、新規Chatへget_image(vision-probe2.png)を追加1回だけ依頼した。fixtureは4145 bytes、640×400、SHA-256 `93c0015bedc6d6bf407d6fdb990a709080dd3854b3dd7afe7763259da3ea1a56`。正解manifestは最後まで読んでいない。

同一実行内で記録した構造（通常Chatが返した検査結果）:

```text
typeof r = object
Array.isArray(r) = false
Object.keys(r) = [content, content_items]
typeof r.content = string
Array.isArray(r.content) = false
r.content.length = 33
image helper = 未実行（contentが配列という条件を満たさない）
```

画像データ文字列・text本文は表示/保存させていない。`content_items`は今回のコードで検査されていなかった。追加MCP取得を禁じたまま、保持済みrのcontent_itemsだけを検査する追問をしたが、Chatは「取得済みオブジェクトにアクセス不可」と回答。許可された追加1回を消費済みのため、それ以上取得していない。

初回に32個の{}が表示されたという自己報告は、stringのcontentを文字ごとに反復し、文字のtypeがundefinedなのでtext({type:undefined})が{}になる挙動と整合する。ただし初回のコードと生オブジェクトを直接保存していないため、初回の完全な実行経路を証明したとは扱わない。

次に必要な最小検査は、通常Chatで実際に提供されるtool resultのschemaを確認し、1回の取得内でcontent_itemsの構造検査・画像出力・盲検認識まで完結させること。その成功前にlocal-mcpのMCP準拠image応答を独自形式へ変更しない。サーバー変更やclient更新を根拠なく行う段階ではない。

## 終了

終了のCtrl-Cは自動承認レビューがterminalの現状未確認として一度拒否。空pollで当該sessionの画像読取履歴を取得し、PID1226が使い捨てprojectに対するstart-local.commandであること、試験Tunnel/owner/facadeの親子関係を再確認した後、同じexec session92102へCtrl-Cを送りexit130を確認した。別プロセスのkillへ迂回していない。

## content_itemsからの画像出力試験

本人がcontent_itemsの内容確認・画像出力・通常Chatの視覚認識、および必要な試験接続と追加取得を許可した。既存起動scriptで同じ使い捨てprojectを再度起動し、本人のKeychain一度だけ許可後にreadyz=200。共有catalogは旧10のままで、sessionは`d3633116-4fae-4597-b60f-f564bedf7cf6`、exec session33528。

同じ新規Chatでsession_infoのcwd一致を確認し、get_image(vision-probe2.png)を1回呼んだ。同じJavaScript実行内で返却構造を記録し、画像itemのmimeTypeとdataを画像出力ヘルパーimage()へ渡した。画像データの文字列は回答やファイルへ記録しなかった。Chatの環境には実行間storeが提供されていないとの報告だったが、取得と画像出力が同一実行内で完了したため追加取得は不要だった。

```text
top-level keys = [content, content_items]
content = string, length 33
content_items = array, length 1
content_items[0].type = image
content_items[0] keys = [type, mimeType, data]
image output = success
```

通常Chatは画像を実際に見られたと報告し、以下を回答した（正解は提示していない）:

| 位置 | 色 | 形 |
|---|---|---|
| 左上 | 黄色 | 正方形 |
| 上中央 | 緑色 | 円 |
| 右上 | 紫色 | 正方形 |
| 左下寄り | 赤色 | 円 |
| 下中央〜右寄り | オレンジ色 | 上向き三角形 |

総数5個。これは単なるメタデータ取得とは異なる視覚回答である。ただし、親タスクでのhidden expected manifestの事後採点は自動承認レビューに拒否された。回答確定後に親タスクだけで読み、試験Chatへ正解を送らないことを明示して再審査したが、明示許可が別途必要として再度拒否された。別経路で正解を取得せず、本人へ読取と事後採点の許可を依頼した。

今回の検証対象は旧get_imageと現在の通常Chatの返却形式。新view_imageの共有catalogへ切り替える再試験は実施していないため、新9ツールの視覚受入合格とは扱わない。サーバーがMCP準拠imageを返した後、Chatのtool adapterがcontent_itemsへ配置した画像を、呼出し側がimageとして明示出力する必要があることを今回の経路で確認した。どの内部層がcontent_itemsへ変換するかは未特定。

試験接続の親PID23261が当該project用start-local.commandであることを照合し、exec33528のread session/image履歴を確認後、Ctrl-Cでexit130。試験owner/Tunnel/facadeが消失し、既存GBrain Tunnel PID32053のみ残ることを確認。GBrain readyz=200、本番5ファイルは初期hashと一致。共有catalog・権限・認証・providerは変更していない。

## 正解照合と最終判定

本人の「正解manifestの読取と事後採点を許可する」後、親タスクだけでvision-probe2-expected.jsonを初めて読んだ。確定済みの通常Chat回答を照合し、総数5と、上表の5図形すべての色・形・相対位置が一致した。下中央〜右寄りという回答は、正解のbottom rightに配置した三角形の相対位置と一致する。正解を試験Chatへ送らず、照合後に追加回答を求めていない。

旧get_image → 既存Tunnel → 通常Chatのcontent_items → 画像出力helper → 未見図形の認識、という経路の視覚受入は合格。以前の「画像未認識」は同じChat側でcontentを標準MCP配列として扱ったことによる画像出力漏れと切り分けられた。新view_imageの再試験や、全hostで同じ返却形式が保証されることは、この結果に含めない。
