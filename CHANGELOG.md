# Changelog

## [0.5.0](https://github.com/diodeme/Gold-Band/compare/v0.4.0...v0.5.0) (2026-06-10)


### Features

* **acp:** cache session titles behind project feature flag ([f39214d](https://github.com/diodeme/Gold-Band/commit/f39214d2273814d3a49d626afea661aecad2c6ed))
* add auto-templates and runtime display status normalization ([cb4b415](https://github.com/diodeme/Gold-Band/commit/cb4b415a01e6da77d666c518b550c369e99bd84a))
* add model selection for workflow nodes and ACP sessions ([4787f6e](https://github.com/diodeme/Gold-Band/commit/4787f6e38b8b8fc65f98d28966f053db907803af))
* add normative permission mode mapping and ACP session mode dropdown ([846b2f8](https://github.com/diodeme/Gold-Band/commit/846b2f83b107133af9648ae31ac56f3e4269a78c))
* add silent critical update on startup ([650341a](https://github.com/diodeme/Gold-Band/commit/650341a0ca989926aa91044698e7427978d129d4))
* add silent critical update on startup ([ef863f7](https://github.com/diodeme/Gold-Band/commit/ef863f7c15c2f7239183827ae247ac80c6f9d508))
* ai dynamic routing ([048febc](https://github.com/diodeme/Gold-Band/commit/048febcfc3fa0bae4d3175ca752b1e9d2f32fefd))
* client metrics reporting — heartbeat, node execution metrics, and settings UI ([8664313](https://github.com/diodeme/Gold-Band/commit/8664313a1f49fc2b81111305c82324e4c0cc7976))
* **conversation:** add per-message file attachments via ACP content blocks ([4663186](https://github.com/diodeme/Gold-Band/commit/4663186e187ffbb3ecef2a235898fbcc4e9bbfcf))
* **conversation:** add shared desktop title bar ([a4544a4](https://github.com/diodeme/Gold-Band/commit/a4544a413dfa1a10d01cf7d113441c8211a650e2))
* **conversation:** attachment upload, preview, validation and drag-drop polish ([c8bf74a](https://github.com/diodeme/Gold-Band/commit/c8bf74a1f917cbc0148a403d2ecb72de1d3d38de))
* **conversation:** polish runtime session chrome ([0f009c7](https://github.com/diodeme/Gold-Band/commit/0f009c73af32c84b66b5206999586f4529f2e4ae))
* **conversation:** polish runtime workflow interactions ([e0003b8](https://github.com/diodeme/Gold-Band/commit/e0003b8dc080e6c56ae98ce2d7c251cc2c8ce095))
* **desktop:** add persistent resizable drawers ([0c8d23b](https://github.com/diodeme/Gold-Band/commit/0c8d23b3133550bf4a7cf2a6e0390cf7930066e6))
* improve new UI ([6569e62](https://github.com/diodeme/Gold-Band/commit/6569e628adb130672ec16e2e2a2a8a8bc6d61758))
* new UI framework for conversational home ([f1277c6](https://github.com/diodeme/Gold-Band/commit/f1277c688b209972fe31d31a5aae04e538992217))
* **runtime:** centralize localized prompt templates ([ecb4232](https://github.com/diodeme/Gold-Band/commit/ecb4232ba385fe81e9a94002334e77313456d1de))
* **storage:** add SQLite search index for cross-session prompt and task retrieval ([f624444](https://github.com/diodeme/Gold-Band/commit/f624444c1d66be4124cf68a7e9ab2cc26db780d6))
* support critical flag in build-channel.mjs via positional arg ([f2a9230](https://github.com/diodeme/Gold-Band/commit/f2a9230939c9c44eb3a21da44dad44f9671f9b41))
* track token usage per session ([#29](https://github.com/diodeme/Gold-Band/issues/29)) ([46e216a](https://github.com/diodeme/Gold-Band/commit/46e216addd8dc11120723578bd0b9be359549882))
* **workflow:** add ai dynamic routing node ([824dae8](https://github.com/diodeme/Gold-Band/commit/824dae8b9af6b9e0d65f569d54b6d9e6d477a1f0))
* **workflow:** add ai-dynamic agent strategies ([4349c60](https://github.com/diodeme/Gold-Band/commit/4349c6086b7dcda13a5a2cb3cd9a82291a3e4e13))
* **workflow:** add ai-dynamic output protocol prompts ([4de4af6](https://github.com/diodeme/Gold-Band/commit/4de4af6cdafeea32bf4c75c42dd38ef05217624a))
* **workflow:** extend AI-DYNAMIC node configuration ([f847289](https://github.com/diodeme/Gold-Band/commit/f847289e92fa145de60b8edc161a7fdd1307cc3c))
* **workflow:** let ai-dynamic fanout own downstream specs ([8c731ec](https://github.com/diodeme/Gold-Band/commit/8c731ec67ef89ccdd39419f02c68d7af72acbcaf))


### Bug Fixes

* **acp:** allow scroll-up during streaming and render new events while reading history ([01458c6](https://github.com/diodeme/Gold-Band/commit/01458c6c792a0d8e1829a39dc94c359b1134b6c0))
* **acp:** eliminate residual scroll jitter and accidental snap-back during streaming ([7dd2086](https://github.com/diodeme/Gold-Band/commit/7dd2086527f964ff83df7506e55029496680c33e))
* **acp:** normalize elapsed time unit labels ([2f177ca](https://github.com/diodeme/Gold-Band/commit/2f177ca42711fe30e4cbd455804a196ec4d5633d))
* **acp:** remove cancel grace period, fix dynamic worker pause, persist fuse, and fix continue button ([c727bc5](https://github.com/diodeme/Gold-Band/commit/c727bc5ca0de81eec79a5272b2fbd2a92a03925d))
* **acp:** resolve follow-up session lifecycle — promptId propagation, optimistic lock, stale status, and direct process kill ([73cbb2f](https://github.com/diodeme/Gold-Band/commit/73cbb2f9c9dc5f91c9545c18a0d25fb1e74fcddc))
* **acp:** stabilize live chat sessions ([9e929e9](https://github.com/diodeme/Gold-Band/commit/9e929e9c5ddd44052262d4270aa66e53630d71a1))
* add missing closing delimiters after get_startup_check_result ([d73837a](https://github.com/diodeme/Gold-Band/commit/d73837a5dec1a46a2a36cc809de1dd02f0fb07e4))
* clean up empty update directory after successful install ([a6ce0b4](https://github.com/diodeme/Gold-Band/commit/a6ce0b4579061f848945c7b4d3b70c2cfb015d30))
* clear pending update bytes before manual install ([e9a8d4b](https://github.com/diodeme/Gold-Band/commit/e9a8d4bb1d0952ca6da381b1af535d82280b939f))
* **conversation:** polish shell and composer surfaces ([e11014c](https://github.com/diodeme/Gold-Band/commit/e11014c934c1e57c5d3404be74f1a9b6854af8b2))
* correct model dropdown data source and improve validation feedback ([fbe0d22](https://github.com/diodeme/Gold-Band/commit/fbe0d22c50f347481725c687d7635ab376054c74))
* delete downloaded update file before install to prevent loop ([5243019](https://github.com/diodeme/Gold-Band/commit/5243019b24f5223f4faefcbf6ed10a29673f7107))
* delete downloaded update file when user manually installs ([ef811f3](https://github.com/diodeme/Gold-Band/commit/ef811f3b6a499ec263774f06728d197f1eb16baa))
* **desktop:** align ACP fonts and preserve agent diagnostics ([588ca23](https://github.com/diodeme/Gold-Band/commit/588ca232a9bdb5bdfe5c1eddd21641828a3ab0c6))
* **desktop:** prevent double .json extension in dynamic node artifact paths ([261f708](https://github.com/diodeme/Gold-Band/commit/261f708e2b85d4392b68bdd82fa1b276b7abd02a))
* **desktop:** unify help tooltip interactions ([cf74f66](https://github.com/diodeme/Gold-Band/commit/cf74f6661a73418c94b41d9e787b1c14152cdf0f))
* eliminate splash event race with check-then-listen pattern ([52b5281](https://github.com/diodeme/Gold-Band/commit/52b5281afeee47db693d4aa9a8fba17142c59f8b))
* prevent silent update loop by comparing versions before download ([9ab9453](https://github.com/diodeme/Gold-Band/commit/9ab945340d9ebc9c373114251edf0ac6fde77f1f))
* prevent theme flash during splash screen startup ([60aa4b9](https://github.com/diodeme/Gold-Band/commit/60aa4b97a6d95a9c5ed496ac5892587322614cc2))
* reduce HTTP requests and clear stale red dot on startup ([2ce17c5](https://github.com/diodeme/Gold-Band/commit/2ce17c5606d4bc933f78a2304270313d0f78791b))
* remove version_is_newer check from background download ([4a3ae70](https://github.com/diodeme/Gold-Band/commit/4a3ae7009d1e91f2fa1c755498a2640613649b42))
* resolve duplicate import and add missing DesktopState startup_check methods ([21dfa6f](https://github.com/diodeme/Gold-Band/commit/21dfa6f88cc798ba62215024e818fe2d74f2e1ff))
* **runtime:** preserve resumable ai-dynamic child runs ([e124873](https://github.com/diodeme/Gold-Band/commit/e1248738a5271e3b03913c2b6144d7a716e4943b))
* set NSIS installMode to currentUser to avoid UAC on update ([1142b1a](https://github.com/diodeme/Gold-Band/commit/1142b1a6c2df6cc77b32d5cbfda879b533643d24))
* use std::env::temp_dir() for cross-platform temp path ([2b9061a](https://github.com/diodeme/Gold-Band/commit/2b9061ab0342cf4e01060e392408ed510c6ac5f7))
* **workflow:** align editor save and validation flows ([1562402](https://github.com/diodeme/Gold-Band/commit/15624026acce644057e78692bafbcad38faecc4f))
* **workflow:** remove default attempt and round limits ([2ed7a2e](https://github.com/diodeme/Gold-Band/commit/2ed7a2e9b2cfdb053c7fb9b1bdef0f9147ce6d92))


### Performance Improvements

* **acp:** eliminate spinner jank and reduce main-thread pressure during streaming ([dcd7e11](https://github.com/diodeme/Gold-Band/commit/dcd7e11c73b1ea6b372815e200058c39391cd13e))
* add timeline/events parse cache and React concurrent rendering ([7e6ce9c](https://github.com/diodeme/Gold-Band/commit/7e6ce9c401b3e6fd85ae9c511db4cb25bc56dd16))

## [0.4.0](https://github.com/diodeme/Gold-Band/compare/v0.3.1...v0.4.0) (2026-05-28)


### Features

* improve updater and built-in profiles ([#21](https://github.com/diodeme/Gold-Band/issues/21)) ([89a5b06](https://github.com/diodeme/Gold-Band/commit/89a5b06c98e8f00aa2b0bbd0998e0782b9569497))
* **workflow:** align default workflow with 测试工作流 ([#26](https://github.com/diodeme/Gold-Band/issues/26)) ([50d6d93](https://github.com/diodeme/Gold-Band/commit/50d6d938014b5ab82ca2bb30f79f8d0d0e15d252))
* **workflow:** improve manual check type and built-in prompt ([#25](https://github.com/diodeme/Gold-Band/issues/25)) ([54219d6](https://github.com/diodeme/Gold-Band/commit/54219d6a1afed672a230ead0bd2f6ee59ceda49a))

## [0.3.1](https://github.com/diodeme/Gold-Band/compare/v0.3.0...v0.3.1) (2026-05-26)


### Bug Fixes

* **agent-management:** surface ACP registry help ([eb70f0d](https://github.com/diodeme/Gold-Band/commit/eb70f0d718dce30691963ea2d877e69b72cb5eda))
* **desktop:** prefer setup.exe in updater manifest, hide ACP child console, surface updater errors ([11a50a2](https://github.com/diodeme/Gold-Band/commit/11a50a2bc38870ac3df70082c4963ce828a23697))

## [0.3.0](https://github.com/diodeme/Gold-Band/compare/v0.2.0...v0.3.0) (2026-05-26)


### Features

* **acp:** add ACP session streaming ([c93b9b2](https://github.com/diodeme/Gold-Band/commit/c93b9b2afec19201e6098e25c9fb1cb35d692ccd))
* **acp:** add cancellation and user-scoped runtime state ([ae3a074](https://github.com/diodeme/Gold-Band/commit/ae3a074fbf47bfee50ba0c32a24da1cbb531001a))
* **acp:** improve session resume and timeline projection ([8ed6b5d](https://github.com/diodeme/Gold-Band/commit/8ed6b5db9eb41e4804dfb5dbbaec291bcc8bac08))
* **acp:** paginate session event history ([d542a9f](https://github.com/diodeme/Gold-Band/commit/d542a9fcf002306bc0767d3cde52461b53d5e4ec))
* **acp:** render agent messages as markdown ([e1a5c5b](https://github.com/diodeme/Gold-Band/commit/e1a5c5b01c417ac79da56db17f4515de56341b33))
* **agent:** support multiple ACP agents ([6b1ab54](https://github.com/diodeme/Gold-Band/commit/6b1ab5433832c5c270317ee63f218c41a80f3dc0))
* **app:** align desktop workflow experience ([956b9f6](https://github.com/diodeme/Gold-Band/commit/956b9f62663eaa0b9b09f006b9a45b5bbf35d734))
* **desktop:** add channelized updater releases ([cb65f4c](https://github.com/diodeme/Gold-Band/commit/cb65f4caf44bc56919a49f16bbee19bd55954e56))
* **ui:** add theme preview selector ([5f493bf](https://github.com/diodeme/Gold-Band/commit/5f493bf1fe46eb20b2e51715ed4df67854393696))
* **ui:** refine appearance preferences ([d2fcd61](https://github.com/diodeme/Gold-Band/commit/d2fcd6197a915408fc0ee3ca9ee71bf8765dba58))
* **ui:** refine appearance preferences ([e79fad4](https://github.com/diodeme/Gold-Band/commit/e79fad4a006ab9da780e600080a51f6b64349724))
* **ui:** refine requirement detail presentation ([932bd1a](https://github.com/diodeme/Gold-Band/commit/932bd1a2047206c764682d8d24cc040e89ab60c2))
* **ui:** refine task orchestration workspace ([d8daba8](https://github.com/diodeme/Gold-Band/commit/d8daba89a511655eccec5f842a309a1fed7c1012))
* **ui:** refine workflow and round detail views ([37eae37](https://github.com/diodeme/Gold-Band/commit/37eae37f99750caf9318a746682053a03ce21f01))
* **ui:** split round activity tabs ([d03a07a](https://github.com/diodeme/Gold-Band/commit/d03a07a2f6ab9e7af563245a18444b5de547e428))
* **workflow:** add manual check and refactor prompt assembly ([1f9e32f](https://github.com/diodeme/Gold-Band/commit/1f9e32f67b07c2dd17d628917607d0fe0fc1d824))
* **workflow:** add profile and template management ([6a1c97f](https://github.com/diodeme/Gold-Band/commit/6a1c97fd9a1abda02963111a4c2795450d62fdb0))
* **workflow:** add task workflow authoring ([9b7f389](https://github.com/diodeme/Gold-Band/commit/9b7f389cced828ab1789740fc73615b4017ae510))
* **workflow:** consolidate execution into worker nodes ([d4c51c6](https://github.com/diodeme/Gold-Band/commit/d4c51c6e7cfb016cae9f71d98564b9c3b147eaa8))
* **workflow:** refine orchestration flow experience ([1c6af6e](https://github.com/diodeme/Gold-Band/commit/1c6af6e8ce223b92a533679267482225c348f2b8))
* **workflow:** repair invalid output without invalid edges ([8e4ce41](https://github.com/diodeme/Gold-Band/commit/8e4ce41846f4ca33b99f9056ce75f6d62dd358c7))
* **workflow:** show last-used template hint ([1afac80](https://github.com/diodeme/Gold-Band/commit/1afac80be33561988e0bc1a44132f30910644b01))
* **workflow:** surface repair attempts and structured errors ([e99f1bd](https://github.com/diodeme/Gold-Band/commit/e99f1bd44bb062446b5b9eeae4a43b3e0adbb0fc))


### Bug Fixes

* **acp:** compact tool rows ([d9e5f44](https://github.com/diodeme/Gold-Band/commit/d9e5f44d9b97c579e9ffe345afaae0c506bfc97c))
* **acp:** improve session chat feedback ([74b01ba](https://github.com/diodeme/Gold-Band/commit/74b01ba4add5a58f3c71f21fbfc6eaa914bf315e))
* **acp:** show tool metadata summaries ([5447993](https://github.com/diodeme/Gold-Band/commit/5447993d45c9a01ef9c86dd5b33eafd379f3cd78))
* **ui:** align orchestration headers and refresh behavior ([a829141](https://github.com/diodeme/Gold-Band/commit/a829141826e8d6b6a331fcdebc240afcec87ec2c))
* **ui:** keep node context across round details ([1415c42](https://github.com/diodeme/Gold-Band/commit/1415c4200bd16f898f24202fe5940c402a05c870))
* **ui:** preserve round detail context ([918ab24](https://github.com/diodeme/Gold-Band/commit/918ab24753d1c675a84b5e4fa4204c38e010c393))
* **ui:** refine requirement detail drawers ([2746189](https://github.com/diodeme/Gold-Band/commit/2746189d10ca03230cd7a8f173e0b55121f4f8af))
* **ui:** separate graph current and selected states ([f221a10](https://github.com/diodeme/Gold-Band/commit/f221a10377656e3000c94ba49d80b967ece5b46e))
* **ui:** simplify settings page copy ([efa8195](https://github.com/diodeme/Gold-Band/commit/efa81951bd25283d997af5ad8d77858e29ec0f42))

## [0.2.0](https://github.com/diodeme/Gold-Band/compare/v0.1.0...v0.2.0) (2026-05-25)


### Features

* **acp:** add ACP session streaming ([c93b9b2](https://github.com/diodeme/Gold-Band/commit/c93b9b2afec19201e6098e25c9fb1cb35d692ccd))
* **acp:** add cancellation and user-scoped runtime state ([ae3a074](https://github.com/diodeme/Gold-Band/commit/ae3a074fbf47bfee50ba0c32a24da1cbb531001a))
* **acp:** improve session resume and timeline projection ([8ed6b5d](https://github.com/diodeme/Gold-Band/commit/8ed6b5db9eb41e4804dfb5dbbaec291bcc8bac08))
* **acp:** paginate session event history ([d542a9f](https://github.com/diodeme/Gold-Band/commit/d542a9fcf002306bc0767d3cde52461b53d5e4ec))
* **acp:** render agent messages as markdown ([e1a5c5b](https://github.com/diodeme/Gold-Band/commit/e1a5c5b01c417ac79da56db17f4515de56341b33))
* **agent:** support multiple ACP agents ([6b1ab54](https://github.com/diodeme/Gold-Band/commit/6b1ab5433832c5c270317ee63f218c41a80f3dc0))
* **app:** align desktop workflow experience ([956b9f6](https://github.com/diodeme/Gold-Band/commit/956b9f62663eaa0b9b09f006b9a45b5bbf35d734))
* **desktop:** add channelized updater releases ([cb65f4c](https://github.com/diodeme/Gold-Band/commit/cb65f4caf44bc56919a49f16bbee19bd55954e56))
* **ui:** add theme preview selector ([5f493bf](https://github.com/diodeme/Gold-Band/commit/5f493bf1fe46eb20b2e51715ed4df67854393696))
* **ui:** refine appearance preferences ([d2fcd61](https://github.com/diodeme/Gold-Band/commit/d2fcd6197a915408fc0ee3ca9ee71bf8765dba58))
* **ui:** refine appearance preferences ([e79fad4](https://github.com/diodeme/Gold-Band/commit/e79fad4a006ab9da780e600080a51f6b64349724))
* **ui:** refine requirement detail presentation ([932bd1a](https://github.com/diodeme/Gold-Band/commit/932bd1a2047206c764682d8d24cc040e89ab60c2))
* **ui:** refine task orchestration workspace ([d8daba8](https://github.com/diodeme/Gold-Band/commit/d8daba89a511655eccec5f842a309a1fed7c1012))
* **ui:** refine workflow and round detail views ([37eae37](https://github.com/diodeme/Gold-Band/commit/37eae37f99750caf9318a746682053a03ce21f01))
* **ui:** split round activity tabs ([d03a07a](https://github.com/diodeme/Gold-Band/commit/d03a07a2f6ab9e7af563245a18444b5de547e428))
* **workflow:** add manual check and refactor prompt assembly ([1f9e32f](https://github.com/diodeme/Gold-Band/commit/1f9e32f67b07c2dd17d628917607d0fe0fc1d824))
* **workflow:** add profile and template management ([6a1c97f](https://github.com/diodeme/Gold-Band/commit/6a1c97fd9a1abda02963111a4c2795450d62fdb0))
* **workflow:** add task workflow authoring ([9b7f389](https://github.com/diodeme/Gold-Band/commit/9b7f389cced828ab1789740fc73615b4017ae510))
* **workflow:** consolidate execution into worker nodes ([d4c51c6](https://github.com/diodeme/Gold-Band/commit/d4c51c6e7cfb016cae9f71d98564b9c3b147eaa8))
* **workflow:** refine orchestration flow experience ([1c6af6e](https://github.com/diodeme/Gold-Band/commit/1c6af6e8ce223b92a533679267482225c348f2b8))
* **workflow:** repair invalid output without invalid edges ([8e4ce41](https://github.com/diodeme/Gold-Band/commit/8e4ce41846f4ca33b99f9056ce75f6d62dd358c7))
* **workflow:** show last-used template hint ([1afac80](https://github.com/diodeme/Gold-Band/commit/1afac80be33561988e0bc1a44132f30910644b01))
* **workflow:** surface repair attempts and structured errors ([e99f1bd](https://github.com/diodeme/Gold-Band/commit/e99f1bd44bb062446b5b9eeae4a43b3e0adbb0fc))


### Bug Fixes

* **acp:** compact tool rows ([d9e5f44](https://github.com/diodeme/Gold-Band/commit/d9e5f44d9b97c579e9ffe345afaae0c506bfc97c))
* **acp:** improve session chat feedback ([74b01ba](https://github.com/diodeme/Gold-Band/commit/74b01ba4add5a58f3c71f21fbfc6eaa914bf315e))
* **acp:** show tool metadata summaries ([5447993](https://github.com/diodeme/Gold-Band/commit/5447993d45c9a01ef9c86dd5b33eafd379f3cd78))
* **ui:** align orchestration headers and refresh behavior ([a829141](https://github.com/diodeme/Gold-Band/commit/a829141826e8d6b6a331fcdebc240afcec87ec2c))
* **ui:** keep node context across round details ([1415c42](https://github.com/diodeme/Gold-Band/commit/1415c4200bd16f898f24202fe5940c402a05c870))
* **ui:** preserve round detail context ([918ab24](https://github.com/diodeme/Gold-Band/commit/918ab24753d1c675a84b5e4fa4204c38e010c393))
* **ui:** refine requirement detail drawers ([2746189](https://github.com/diodeme/Gold-Band/commit/2746189d10ca03230cd7a8f173e0b55121f4f8af))
* **ui:** separate graph current and selected states ([f221a10](https://github.com/diodeme/Gold-Band/commit/f221a10377656e3000c94ba49d80b967ece5b46e))
* **ui:** simplify settings page copy ([efa8195](https://github.com/diodeme/Gold-Band/commit/efa81951bd25283d997af5ad8d77858e29ec0f42))
