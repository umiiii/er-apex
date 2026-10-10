# er-apex

[English](README.md) | 中文

在《艾尔登法环》里扮演《Apex 英雄》动力小子（Octane）的离线趣味模组。Rust 编写的 DLL，由 [me3](https://me3.help) 加载，架构参考 [er-mario](https://github.com/deltarooo/er-mario)。

**本仓库不附带任何游戏文件或游戏资源**，包括《Apex 英雄》和《艾尔登法环》的模型、贴图、动画、音效、HUD 图片和文本。这些资源用本仓库 `tools/` 下的转换工具，从你自己安装的游戏里生成（见[编译](#编译)）。《Apex 英雄》的安装目录必须由你自己指定。

## 这个 fork 的改动

本仓库 fork 自 [umiiii/er-apex](https://github.com/umiiii/er-apex)，在原版基础上：

**武器**

- **六把枪**：Wingman（开局拿在手上）、R-99、R-301、平行步枪、哨兵、Charge Rifle。
- **哨兵**（特意改过）：全自动每 0.8 秒一发，带 Apex 拉栓动作，弹匣 7 发，每 0.4 秒自动补 1 发（不用换弹），用充能后的开火音效，每发命中都算爆头（70 × 1.8，击杀栏带爆头标记）；**子弹自动追踪**：面朝方向 60 米、左右各 30° 的锥形范围内有敌人时，开枪打出一发发光的追踪子弹，从枪口飞出、拐弯追着敌人飞，飞到才算命中，准星不会动（ini `homing`、`homing_range`、`homing_angle`、`homing_height`、`homing_speed` 可改，默认值写在 `src/spike/homing.rs` 开头）。第一人称模型、动画、音效、HUD 图标都由 `export-assets.ps1` 第 8 步从本机 Apex 导出。
- **武器轮盘**：按住 Tab，转视角（或按 1～3）选枪，松开就换。按 2 仍是 Charge Rifle。
- **默认模型和贴图**：所有枪都用默认模型和默认贴图，每个人的 Apex 里都有。R-99 的刀锋模型和平行步枪的青色热情模型需要手动开启（`export-assets.ps1 -SkinModels`），下面的皮肤也是可选的。
- **检视动作**：按 5 播放手中武器的检视和音效，开火、开镜、换弹、切枪、冲刺或用技能会打断。
- **收枪模式**：按 3 收起枪，换上恶灵的传家宝苦无（第一人称模型和动作都从本机 Apex 导出）；移速稍快（×1.1，ini `holster_speed`），鼠标左键挥刀（2 米内 30 伤害），5 播放检视（都带苦无音效），再按 3（或 1 / 2）切回枪。HUD 保持不变。
- **Wingman**：半自动、8 发、单发 50、爆头 ×1.5、换弹 2.1 秒、开镜 60°，后坐力用 Apex 的数值。**Charge Rifle**：8 发。
- **自定义皮肤（可选）**：把自己的贴图放进 `apex-data\skins\`，第 8 步会自动用上（见[武器皮肤](#武器皮肤可选)）。仓库里不含任何皮肤贴图。

**HUD 字体（可选）**

- 把自己的 `.ttf` / `.otf` 放进 `apex-data\fonts\`，HUD 的数字和英文就用这些字体显示（中文等其他字符仍用 Apex 原字体）。默认所有文字用 `Apex Regular`（没有时用第一个字体）；ini 的 `hud_font_body`、`hud_font_numeric`、`hud_font_bold`（填字体文件名去掉扩展名的小写，或 `off`）可以分别指定，`hud_font = off` 全部关闭。字形图集由 `pwsh export-assets.ps1`（第 8 步）或 `python tools/apexhud/custom_font.py` 生成。仓库里不含字体文件。

**游玩**

- **F5**：切回你自己的艾尔登法环角色，移动和碰撞、视角、HUD、护甲和武器都用游戏原生的，模组的枪、技能和 HUD 关闭。再按 F5 切回动力小子。
- **键鼠按键提示**：默认游戏看不到任何手柄，界面提示是键盘鼠标。要用手柄玩：`pwsh play.ps1 -Pad`。
- **敌人判定**：除玩家一方（阵营 1、2、8、12，可用 ini `friendly_teams` 改）外所有角色都能被击中，和原版一样，狗、乌鸦、入侵者、龙和 Boss 都算。
- **移动**：每秒重读地面碰撞，下落中陷进地面时拉回，避免远处地图晚加载时穿地；坐电梯上下时由游戏接管，直到电梯停稳。
- **探路者钩爪**：武器轮盘（按住 Tab）里也能选 Q 技能：兴奋剂或钩爪。选钩爪时按 Q 射出钩子（最远 850 单位，约 21 米），按 Apex 原版参数把你拉过去（速度 1.5 秒内从 50 升到 800、加速度 1500、往上拉时重力变轻、脱钩时补一点向上的速度）；按空格、再按 Q、到达钩点或速度太慢时脱钩。无冷却，用 Apex 原版音效，画出钩索（暂时没有第一人称手部动作）。
- **连杀段位徽章**（右上角，Apex 显示段位的位置）：最近 1 分钟内击杀 0~3 显示 D1，4~10 显示 M1，超过 10 显示 P1。徽章用的是 Apex 游戏内的钻石、大师、Apex 猎杀者段位标，在第 8 步从本机游戏里解包（`tools/apexhud/export_rank.py`）；如果 `apex-data\hud\rank\` 里放了自己的 `D1.png`、`M1.png`、`P1.png`，就优先用自己的。
- **击杀栏名字**：怪物显示游戏里这一类怪的名字（取自同种骨灰的中文名），游戏里没有的用社区通俗叫法（`tools/apexhud/monster_names_zhocn.json`，按 Paramdex 英文名对照），对照表里也没有的用英文名，都没有才显示“敌人”。
- **命中音效**：第一次打中某个敌人并造成伤害时播放 Apex 的碎甲音效，每个敌人只播一次（`--set hits`）。
- **Charge Rifle 音效**随射击结束而停：打断后不再有开火声，光束打到怪物身上也不会循环。
- **设置**：`pwsh play.ps1 -Fov 90 -FpsLimit 120 -PlayerName "名字" -Pad`。FOV 默认 90；帧数默认原版 60；HUD 名字取 `-PlayerName` 或环境变量 `ER_APEX_PLAYER_NAME`，都没有时用存档角色名。

**部署**

- **没装 DLC 也能导出**：没有《黄金树幽影》时 `erextract` 跳过 `DLC.bhd`。
- `export-assets.ps1` 共八步（第 8 步：Wingman、R-99、苦无和皮肤），按下面的流程运行一次即可。

已知限制：第三人称身体手里仍是 R-301；Wingman 击锤不单独动作；自发光贴图未使用。

## 极简开始

复制这段 markdown 给任意 agent：

```markdown
帮我把 https://github.com/MegaWeed/er-apex 仓库跑起来
```

## 开始使用

### 准备工具链

**系统与磁盘**

- Windows 10/11 x64，PowerShell 7（`pwsh`）。
- 已安装《艾尔登法环》（游戏绑定支持 2.7.1.0 / 2.7.1.1）和《Apex 英雄》。
- 仓库路径尽量短（例如 `D:\er-apex`）：导出的文件路径很深，系统没开长路径支持时，路径太长会出错。
- 生成的资源约占 45 GB 磁盘空间。
- 第一次生成资源需要联网：要下载 RSX 源码与解码库、`tools/erdata` 的固定版本依赖、NuGet 包和 Rust crate。

**要安装的软件**

| 软件 | 版本与说明 |
|---|---|
| Python | 3.11 以上（实测 3.13）。第三方包只有三个：`pip install numpy pillow matplotlib` |
| Rust | stable，MSVC 工具链（`x86_64-pc-windows-msvc`，见 `rust-toolchain.toml`）；编译 DLL、`deps/` 下的 crate 和 `tools/erdata/erextract` |
| .NET SDK | 8.0.425 或同一主版本的更新补丁（`tools/erdata`、`tools/testarena` 的 `global.json` 锁定）；编译 `ertool` 和几个 C# 小工具 |
| Visual Studio 2022 | 安装「使用 C++ 的桌面开发」工作负载（含 MSVC 和 Windows 10/11 SDK）；`build_rsx.py` 用 MSBuild 编译 RSX，Rust 的 MSVC 链接也要用它 |

**自己下载放进 `tools/` 的三个程序**（不入库）

| 程序 | 放到 | 来源 |
|---|---|---|
| me3 | `tools\bin\me3\bin\me3.exe` | [me3.help](https://me3.help) 的 Windows 发行包，解压后把整个目录放到 `tools\bin\me3\` |
| texconv | `tools\bin\texconv\texconv.exe` | [DirectXTex](https://github.com/microsoft/DirectXTex/releases) 发行版里的 `texconv.exe` |
| RSX 2.3.0 | `tools\rsx-2.3.0\rsx_nogui.exe` | [r-ex/rsx](https://github.com/r-ex/rsx/releases) 的 2.3.0 发行包，解压到 `tools\rsx-2.3.0\`。只有 `rsx_export.py` 用它，其余导出用 `build_rsx.py` 从源码编译的改版 |

### 1. 编译 DLL

```powershell
pwsh build.ps1          # → target\x86_64-pc-windows-msvc\release\er_apex.dll
pwsh build.ps1 -Test    # 同时运行 deps/ 下各 crate 的测试
```

### 2. 生成游戏资源

```powershell
pwsh export-assets.ps1
```

- 脚本会询问《Apex 英雄》的安装目录（里面有 `paks\Win64`）和《艾尔登法环》的 `Game` 目录（里面有 `eldenring.exe`）。能从 Steam 找到时会作为默认值显示，直接回车即可。两个目录只读取，不修改。
- 然后检查工具链，按顺序执行[生成游戏资源](#生成游戏资源apex-dataer-data)的八个步骤，约 70 分钟。第 4 步会启动一次游戏（约 1 分钟）读取玩家骨架，游戏自己退出之前不要操作它。
- 产物已存在的命令会跳过，所以中途失败时排除原因后直接重新运行即可，会从停下的地方继续；上次被中断的命令会重新完整执行。全部重新生成：`-Force`；只重做第 5–8 步：`-From 5 -Force`。不想回答问题：`-ApexDir <目录> -EldenRingDir <目录>`。不生成测试场地（没有士兵当靶子）：`-NoArena`。
- 武器皮肤是可选的：在这一步之前或之后放进 `apex-data\skins\` 都行（见[武器皮肤](#武器皮肤可选)）。

### 3. 启动游戏

```powershell
pwsh play.ps1
```

- 脚本把测试存档备份到 `scratch\saves`，把模组装到 `scratch\mod`，再通过 me3 以 1920×1080 窗口离线启动游戏。游戏会跳过标题画面，「继续」测试存档里最后玩的角色，站在宁姆格福「引导之始」赐福旁：第一人称的动力小子，手持 Wingman，赐福旁刷出三名士兵当靶子。
- 测试存档 `ER0000_fuse.sl2` 里要至少有一个角色。第一次用 me3 启动时，me3 会从正常存档复制一份。
- **点一下游戏窗口**就能用键盘鼠标玩。不加 `-Pad` 时游戏看不到手柄。
- 按键：WASD 移动、空格跳、Shift 冲刺、Ctrl 蹲 / 滑铲；鼠标左键开火、右键开镜、R 换弹；按住 Tab 开武器轮盘（或按 1～3），2 切 Charge Rifle，3 收枪模式（苦无），5 检视；Q 兴奋剂、Z 跳板、4 护盾电池、G 破片手雷；**F5** 切回艾尔登法环角色，再按切回来。
- 选项：`-Fov 90`（70～110）、`-FpsLimit 120`（默认原版 60）、`-PlayerName "名字"`（HUD 显示的名字）、`-Pad`（用手柄）。
- 默认枪伤害 ×3（Charge Rifle 不吃这个加成，保持原伤害）、跳板无冷却；加 `-Season3` 用 S3 原值。`-NoSpawn`：不刷士兵。
- 再刷士兵：`pwsh tools/dev/game.ps1 spawn`。赐福旁约 10.5 m 有一名友方 NPC，别朝它开枪。回到赐福：`pwsh tools/dev/game.ps1 cmd "warp 1042361951"`。退出：`pwsh tools/dev/game.ps1 stop`。

## 编译

`build.ps1` 和 `export-assets.ps1` 执行的就是本节内容。这里的命令供手动执行其中某一部分时参考。

### DLL

依赖的三个自写 crate 都在本仓库的 `deps/` 下：`er-apex-move`（Apex 移动控制器）、`er-apex-audio`（混音播放）、`er-apex-anim`（第一人称动画包格式，生成资源第 7 步的 `export_anim.py` 会调用它）。单独克隆本仓库就能编译 DLL。

```powershell
cargo build --release
# 产物：target\x86_64-pc-windows-msvc\release\er_apex.dll

# 三个 crate 的测试
cargo test --release --offline --manifest-path deps/er-apex-move/Cargo.toml
cargo test --release --offline --manifest-path deps/er-apex-anim/Cargo.toml
cargo test --release --offline --manifest-path deps/er-apex-audio/Cargo.toml
```

### 生成游戏资源（`apex-data/`、`er-data/`）

模组用到的资源，都由 `tools/` 下的转换工具从本机安装的游戏生成，不入库（`apex-data/`、`er-data/`、`scratch/` 已在 `.gitignore` 里）。下面的命令已在一份空的副本里按顺序完整跑通过，生成的资源进游戏验证可用。各工具的细节和验证脚本（`verify_*.py`）见 `tools/` 下各目录的 README。

**游戏目录**

转换工具从环境变量读取两个游戏目录，只读取，不修改：

| 变量 | 指向 | 是否必填 |
|---|---|---|
| `APEX_LEGENDS_DIR` | 《Apex 英雄》安装目录（里面有 `paks\Win64`） | **必填**，没有默认值 |
| `ELDEN_RING_DIR` | 《艾尔登法环》的 `Game` 目录（里面有 `eldenring.exe`） | 不同于 `E:\SteamLibrary\steamapps\common\ELDEN RING\Game` 时必填 |

```powershell
$env:APEX_LEGENDS_DIR = 'D:\SteamLibrary\steamapps\common\Apex Legends'   # 改成你的安装目录
$env:ELDEN_RING_DIR   = 'D:\SteamLibrary\steamapps\common\ELDEN RING\Game'
# 想长期保存：[Environment]::SetEnvironmentVariable('APEX_LEGENDS_DIR', '<目录>', 'User')
```

没设置 `APEX_LEGENDS_DIR`，或者目录里找不到 `paks\Win64`，转换工具会直接报错并提示怎么设置。

不需要 R5Reloaded。HUD、手雷、音效的导出会核对 S3 脚本中哪一行用到了某个名字，这些出处（文件名、行号和对应的短值，不含脚本内容）已记录在 `tools/s3_evidence.json` 里，随仓库提供。资源本身全部来自你的《Apex 英雄》正式服。维护者需要重建记录时，把 `R5RELOADED_RECORD` 设为 R5Reloaded 的 `LIVE` 目录，再运行这三个导出（见 `tools/s3record.py`）。

所有命令都在本仓库的根目录执行，按顺序进行，后面的步骤要用前面的产物。括号里的耗时是实测值。

有几步会用到名字里带 `fuse`（暴雷）的产物：这个项目原来扮演暴雷，后来改成动力小子。Apex 骨架、R-301 第三人称模型、R-301 第一人称动作包和第一人称基础姿态，最初都是随暴雷一起做的，动力小子的转换仍以它们为输入。下面只生成这些必需的部分，不生成暴雷自己的身体、HUD 和音效。

**1. Apex 导出（约 31 分钟）**

```powershell
python tools/apexassets/build_rsx.py                     # 编译改版 RSX（首次，需要网络）
python tools/apexdata/rsx_export.py                       # 角色设置、武器定义、本地化 → apex-data\export
python tools/apexdata/extract_fuse.py                     # 武器参数表 apex-data\fuse_data.json（音效导出要用）
python tools/apexassets/export_assets.py                  # 共享输入：Apex 骨架、R-301 第三人称模型、第一人称手臂（T001）
python tools/apexassets/export_assets.py --legend octane  # 动力小子身体、手臂、注射器、跳板（T014）
python tools/apexpov/export_pov.py                        # R-301 第一人称模型与动作、原始 RSEQ、QC/SMD → apex-data\pov
python tools/apexpov/export_defender_pov.py               # 充能步枪第一人称 QC/SMD
python tools/apexassets/frag_grenade_assets.py            # 破片手雷
python tools/apexassets/battery_assets.py                 # 护盾电池（T021）
python tools/apexassets/defender_assets.py                # 充能步枪（T022）
python tools/apexassets/frag_assets.py                    # 手雷与投掷物（T022）
```

**2. HUD 与音效（约 5 分钟）**

```powershell
python tools/apexhud/export_hud.py --legend octane      # → apex-data\hud\octane（T017）
python tools/apexhud/export_extra.py --legend octane    # 补充图片，含手雷图标（U9）
python tools/fuseaudio/export_audio.py                  # R-301 → apex-data\audio
python tools/fuseaudio/export_audio.py --set octane     # 技能与语音（T018）
python tools/fuseaudio/export_audio.py --set defender   # 充能步枪
python tools/fuseaudio/export_audio.py --set frag       # 破片手雷
```

**3. 艾尔登法环提取（约 2 分钟）**

```powershell
Push-Location tools/erdata
python scripts/setup_dependencies.py                     # 下载固定版本的依赖源码并打补丁
cargo build --release --manifest-path erextract/Cargo.toml
dotnet build ertool/ertool.csproj -c Release
python scripts/run_s2a.py              # c0000 骨架、护甲模板、文本 → er-data\extract、er-data\json（T002）
python scripts/s3a_roundtrip.py        # 护甲构建器往返、原版网格与材质包 → er-data\s3（T003）
python scripts/s3a_material_evidence.py
Pop-Location

# 击杀信息用的 NPC 名字表
$fmg = Get-ChildItem er-data\extract\msg\zhocn\item.msgbnd -Recurse -Filter NpcName.fmg | Select-Object -First 1
& tools\erdata\ertool\bin\Release\net8.0\ertool.exe fmg $fmg.FullName --json --out er-data\json\NpcName_zhocn.json

python tools/testarena/run.py build    # 可选：测试场地 → er-data\test_arena（T013，见下）
```

`testarena` 是开发测试用的：它从你本机的原版地图改出「引导之始」的地图与事件文件，在赐福旁加一个敌人生成器，`game.ps1 spawn` 时刷出三名士兵当靶子。正常游玩用不到，可以跳过这一行；跳过时 `game.ps1 install` 会提示找不到测试场地，`spawn` 不起作用，也可以给 `install` 加 `-NoArena`。

**4. 游戏运行时的骨架转储（约 1 分钟）**

骨骼映射和第一人称道具要用游戏运行时的玩家骨架。这一步进一次游戏导出骨架，此时还没有动力小子的模型，不要开 `first_person`。

```powershell
$root = (Get-Location).Path
pwsh tools/dev/game.ps1 build
pwsh tools/dev/game.ps1 install -Set "quickboot = 1;qb_place = first_step"
pwsh tools/dev/game.ps1 start
$log = "$root\scratch\mod\logs\er_apex.log"
while (-not ((Test-Path $log) -and (Select-String -Quiet 'quickboot: done' $log))) { Start-Sleep 1 }
pwsh tools/dev/game.ps1 cmd "skeleton c0000_runtime.json"    # 写到 scratch\mod\dev\
pwsh tools/dev/game.ps1 stop
New-Item -ItemType Directory -Force er-data\runtime, er-data\skeleton | Out-Null
Copy-Item scratch\mod\dev\c0000_runtime.json er-data\runtime\c0000_runtime.json
Copy-Item scratch\mod\dev\c0000_runtime.json er-data\skeleton\c0000_live_skeleton.json

python tools/retarget/skeleton_map.py    # Apex 骨 → ER 骨的映射 → er-data\s2b\mapping-v0.json
```

**5. 身体模型 999（约 2.5 分钟）**

```powershell
python tools/octanemesh/convert_octane.py --matbin-bnd er-data\s3\inputs\material\allmaterial.matbinbnd.dcx   # → er-data\s3\octane（T015）
```

**6. 第一人称模型 998、手持 R-301 与动画包（约 17 分钟）**

每一步都叠加在上一步之上：材质包（`allmaterial.matbinbnd.dcx`）和动画包（`fuse_pov.anim`）都以上一步的产物为底，必须按顺序执行。

```powershell
python tools/apexpov/bake_pov.py                    # 共享输入：R-301 第一人称动作包 → apex-data\pov\fuse_pov.anim（T012）
python tools/fusepov/build_pov.py --legend octane --matbin-bnd er-data\s3\octane\package\material\allmaterial.matbinbnd.dcx   # 手臂 + R-301（T016）
python tools/apexpov/bake_pov.py --legend octane    # → apex-data\pov\octane\fuse_pov.anim
python tools/fusegun/build_gun.py --legend octane   # 999 右手拿 R-301（T019）
python tools/apexpov/bake_ability.py                # 注射器、手持跳板（T020）
python tools/fusepov/build_ability.py
python tools/apexpov/bake_battery.py                # 护盾电池（T021）
python tools/fusepov/build_battery.py
python tools/apexpov/bake_weapons.py                # 充能步枪、手雷（T022）
python tools/fusepov/build_weapons.py               # 电池改蓝（battery_tint.py）也在这一步
python tools/apexpov/bake_padworld.py               # 地上跳板动画（R4）
```

**7. 第一人称基础姿态（约 1 分钟）**

第一人称每帧先播放一个基础姿态 `fuse_idle_rifle_ADS`，视图模型靠这套姿态钩子写到骨骼上。缺了它，第一人称的手和枪不会显示在正确位置。

```powershell
python tools/fuseanim/export_anim.py                # Apex 第三人称动作包（T006）→ apex-data\anim\fuse.anim
python tools/fusemesh/convert_fuse.py --geometry-only   # 只算骨架对齐 → er-data\s3\fuse\align.json，不生成模型
python tools/retarget/bake_er_anim.py fuse_idle_rifle_ADS   # → er-data\s4\fuse_er.anim
```

**8. Wingman、R-99、平行步枪、哨兵与苦无（约 20 分钟，本 fork 新增）**

和前面几步一样，从本机 Apex 导出 Wingman、R-99、平行步枪、哨兵和恶灵的传家宝苦无，都是默认模型。`build_wingman.py` 在第 6 步 `octane_pov_weapons` 的基础上，把它们加进模型 998 和动画包，并套用可选的皮肤。

```powershell
python tools/apexassets/wingman_assets.py
python tools/apexassets/r99_assets.py
python tools/apexassets/flatline_base_assets.py       # 平行步枪（flatline_base_v）
python tools/apexassets/kunai_assets.py               # 恶灵传家宝苦无（heirloom_wraith_v18_kunai_v）
python tools/apexassets/sentinel_assets.py            # 哨兵（sentinel_base_v）
python tools/apexpov/bake_wingman.py                  # → apex-data\pov\octane_wingman\fuse_pov.anim
python tools/fusepov/build_wingman.py                 # → er-data\s3\octane_pov_wingman（模型 998）；皮肤见下
python tools/fuseaudio/export_audio.py --set wingman
python tools/fuseaudio/export_audio.py --set r99
python tools/fuseaudio/export_audio.py --set kunai
python tools/fuseaudio/export_audio.py --set flatline
python tools/fuseaudio/export_audio.py --set sentinel
python tools/fuseaudio/export_audio.py --set hits
python tools/fuseaudio/export_audio.py --set grapple
python tools/apexhud/export_wingman.py --legend octane
python tools/apexhud/export_wingman.py --legend octane --weapon r99
python tools/apexhud/export_wingman.py --legend octane --weapon flatline
python tools/apexhud/export_wingman.py --legend octane --weapon sentinel
python tools/apexhud/export_wingman.py --legend octane --weapon kunai
python tools/apexhud/export_wingman.py --legend octane --weapon grapple
python tools/apexhud/export_rank.py --legend octane    # 连杀徽章：Apex 的段位标
python tools/apexhud/custom_font.py                   # 仅当 apex-data\fonts 里有字体：→ apex-data\hud\custom_font
```

#### 皮肤模型（可选）

`pwsh export-assets.ps1 -SkinModels` 会把 R-99 换成刀锋模型（`r99_react_v20_ascension_v`）、平行步枪换成青色热情模型（`flatline_v20_trshunter_v`），并重新生成动画包和模型 998；不带 `-SkinModels` 再运行一次就换回默认模型。手动执行时，多导出这两个模型，并给 `bake_wingman.py` 和 `build_wingman.py` 设置 `ERAPEX_SKIN_MODELS=1`：

```powershell
python tools/apexassets/r99_ascension_assets.py
python tools/apexassets/flatline_assets.py
$env:ERAPEX_SKIN_MODELS = '1'
python tools/apexpov/bake_wingman.py
python tools/fusepov/build_wingman.py
```

#### 武器皮肤（可选）

`apex-data\skins\` 下放了哪个文件夹，就替换哪把枪的贴图（放在别处：`export-assets.ps1 -Skins <文件夹>`）：

| 文件夹 | 文件 | 替换 |
|---|---|---|
| `apex-data\skins\wingman\` | `Wingman_Default_col.dds`（可选 `_spc`、`_nml`、`_gls`；`.png` 也行） | Wingman 的基础材质 |
| `apex-data\skins\chargerifle\` | `col\`、`nml\`、`gls\` 子文件夹，各含若干尺寸的 `.dds`，如 `1024.dds`、`2048.dds`，取最大的一张 | Charge Rifle 的主材质 |
| `apex-data\skins\r99\` | `<尺寸> COL SPC.dds`，如 `2048 COL SPC.dds`，取最大的一张 | R-99 的颜色和高光（刀锋模型：仅 `-SkinModels`） |
| `apex-data\skins\flatline\` | `*COL*.dds` 和 `*SPC*.dds`（可选 `*AO*.dds` 发光层），取最大的一张 | 平行步枪的颜色和高光（青色热情模型：仅 `-SkinModels`） |
| `apex-data\skins\kunai\` | `*_col.dds` 和 `*_spc.dds`（放在子文件夹里也行，如 `1024\P2020_Default_col.dds`），取最大的一张 | 苦无的颜色和高光 |

添加、更换或删除皮肤文件夹后，再运行一次 `pwsh export-assets.ps1`：第 8 步发现皮肤变了，会重新生成模型 998，其余步骤跳过。也可以手动执行：

```powershell
python tools/fusepov/build_wingman.py --skin apex-data\skins\wingman --cr-skin apex-data\skins\chargerifle --kunai-skin apex-data\skins\kunai
```

全部完成后用 `pwsh play.ps1` 启动游戏。`game.ps1 install -Legend octane` 会取各链条最后一级的产物：999 用 `octane_gun`，998、材质包和动画包用 `octane_pov_wingman`（没有时用 `octane_pov_weapons` / `octane_weapons`），另外复制基础姿态 `fuse_er.anim` 和地上跳板 `padworld.json`。某一级不完整时退回上一级。

生成完的 `apex-data` 和 `er-data` 大部分是中间产物。游戏实际读取的部分约 490 MB：模型约 147 MB，动画包约 61 MB，HUD 约 45 MB，音效约 235 MB。

## 致谢

- [er-mario](https://github.com/deltarooo/er-mario)（Delta）：本模组的架构来源。`tools/erdata/erextract` 的归档、DCX、BND4 读取改编自它（MIT）。
- [fromsoftware-rs](https://github.com/vswarte/fromsoftware-rs)：《艾尔登法环》游戏绑定，固定提交与 er-mario 相同。
- [me3](https://me3.help)：模组加载器。
- [hudhook](https://crates.io/crates/hudhook)、[ilhook](https://crates.io/crates/ilhook)、[pelite](https://crates.io/crates/pelite)、[glam](https://crates.io/crates/glam) 以及 `Cargo.toml` 里的其他 crate。
- [RSX](https://github.com/r-ex/rsx)（r-ex）：《Apex 英雄》资源导出（2.3.0 发行版与从源码编译的改版）。
- [SoulsFormatsNEXT](https://github.com/soulsmods/SoulsFormatsNEXT)（原作者 Joseph Anderson，GPL-3.0）与 [HKLib](https://github.com/The12thAvenger/HKLib)（MIT）：`ertool`、`testarena` 用到的《艾尔登法环》文件格式。
- [Paramdex](https://github.com/soulsmods/Paramdex) 与 [UXM Selective Unpack](https://github.com/Nordgaren/UXM-Selective-Unpack)：参数定义与归档文件名字典，只在本机使用。
- [DirectXTex](https://github.com/microsoft/DirectXTex)（`texconv`）：贴图转换。

转换工具依赖的许可与固定提交见 `tools/erdata/THIRD_PARTY_NOTICES.md`、`tools/testarena/THIRD_PARTY_NOTICES.md` 和 `tools/erdata/dependencies.lock.json`。

《Apex 英雄》属于 Respawn Entertainment 与 Electronic Arts，《艾尔登法环》属于 FromSoftware 与 Bandai Namco。本项目是粉丝作品，与以上公司无关。本仓库只含源码，不含任何游戏文件或游戏资源；生成的资源仅供你本人在自己拥有的游戏副本上使用，请勿再分发。

许可证：MIT（见 `LICENSE`）。`tools/erdata/ertool` 与 `tools/testarena` 因使用 SoulsFormatsNEXT 按 GPL-3.0 发布；它们是开发工具，不链接进模组。
