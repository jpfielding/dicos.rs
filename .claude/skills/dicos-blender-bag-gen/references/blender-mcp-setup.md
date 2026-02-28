# Blender MCP Setup

Complete setup guide for connecting Claude Code to Blender via the [blender-mcp](https://github.com/ahujasid/blender-mcp) Model Context Protocol server.

## 1. Install uv

The MCP server runs via `uvx` (from the [uv](https://docs.astral.sh/uv/) package manager).

**macOS:**
```bash
brew install uv
```

**Linux:**
```bash
curl -LsSf https://astral.sh/uv/install.sh | sh
```

Verify:
```bash
uvx --version
```

## 2. Install the Blender Addon

1. Download `addon.py` from the repository:
   ```bash
   curl -LO https://raw.githubusercontent.com/ahujasid/blender-mcp/main/addon.py
   ```

2. Open Blender

3. Go to **Edit > Preferences > Add-ons**

4. Click **Install from Disk** (top-right dropdown or button, varies by Blender version)

5. Select the downloaded `addon.py` file

6. Enable the addon by checking the box next to **"Interface: Blender MCP"**

## 3. Configure Claude Code MCP Server

Add the blender MCP server to your Claude Code config. Either run:

```bash
claude mcp add blender -- uvx blender-mcp
```

Or manually edit `~/.claude.json` and add to the `mcpServers` section:

```json
{
  "mcpServers": {
    "blender": {
      "command": "uvx",
      "args": ["blender-mcp"]
    }
  }
}
```

This can be added at the global level (top-level `mcpServers`) or per-project (under `projects > "/path/to/project" > mcpServers`).

## 4. Activate in Blender

1. In Blender, open the sidebar in the 3D Viewport (press **N**)
2. Find the **"BlenderMCP"** tab in the sidebar
3. Click **"Start MCP Server"** (or toggle "Connect to Claude")
4. The server status should show as connected

## 5. Verify Connection

In Claude Code, test the connection:

```
> are you connected to blender mcp?
```

Claude should be able to call `mcp__blender__get_scene_info` and return scene details. If it fails, check:
- Blender is open with the MCP addon enabled and server started
- `uvx blender-mcp` runs without error in a terminal
- `~/.claude.json` has the correct `mcpServers` entry
- Restart Claude Code after config changes

## Available MCP Tools

Once connected, these Blender tools are available:

| Tool | Description |
|------|-------------|
| `mcp__blender__get_scene_info` | List all objects, materials, scene metadata |
| `mcp__blender__get_object_info` | Details on a specific object (bounds, materials, vertices) |
| `mcp__blender__get_viewport_screenshot` | Capture viewport as image |
| `mcp__blender__execute_blender_code` | Run arbitrary Python in Blender |

All scene construction in this skill uses `execute_blender_code` with `bpy` Python scripts.
