# Threat Items Catalog

Comprehensive catalog of TSA-prohibited items for CT scan simulation, organized by DICOS Threat Category. Each item includes Blender geometry, density, and CT scan signature description.

## DICOS Threat Categories

Per NEMA IIC 1 v04-2023, the Threat Category attribute (4010,1012) uses these Defined Terms:

| Threat Category | Description |
|-----------------|-------------|
| ANOMALY | Suspicious configuration, unknown threat |
| CONTRABAND | Prohibited items (tools, sporting goods, non-explosive weapons) |
| EXPLOSIVE | Explosives, IED components, detonators |
| LAPTOP | Electronics (used for virtual removal on PVS/SVS) |
| PHARMACEUTICAL | Drugs, controlled substances |
| PI | Personally Identifiable (privacy-related detection) |
| METAL | Metal threat objects (firearms, knives, blades) |
| NONMETAL | Non-metallic threats (ceramic knife, 3D-printed weapon) |
| UNKNOWN | Unclassified threat |
| OTHER | Catch-all for uncategorized threats |

Assessment Flag values: `HIGH_THREAT`, `THREAT`, `NO_THREAT`, `UNKNOWN`

See [tdr-workflow.md](tdr-workflow.md) for how to create TDR bounding boxes from these items.

## Firearms (Threat Category: METAL)

Distinctive CT signature: dense L-shaped or rectangular metal mass, barrel as cylinder, magazine as dense rectangle, spring mechanism visible.

| Item | Object Names | Geometry | Material | CT Signature |
|------|-------------|----------|----------|--------------|
| Handgun/pistol | CT_Gun_Frame, CT_Gun_Barrel, CT_Gun_Grip, CT_Gun_Magazine | Frame: L-cube 0.08x0.04x0.06; Barrel: cyl r=0.006 d=0.08; Grip: cube 0.03x0.02x0.06; Magazine: cube 0.02x0.015x0.04 | CT_Threat_Metal (20000) | Very dense L-shape, barrel bore visible as air channel inside cylinder |
| Revolver | CT_Revolver_Frame, CT_Revolver_Barrel, CT_Revolver_Cylinder | Frame: L-cube; Barrel: cyl r=0.007 d=0.06; Cylinder: cyl r=0.018 d=0.03 | CT_Threat_Metal | Dense L-shape with distinctive rotating cylinder |
| Disassembled firearm | CT_GunPart_Slide, CT_GunPart_Frame, CT_GunPart_Barrel | Separate pieces spread through bag | CT_Threat_Metal | Multiple dense metal components, harder to detect when separated |
| Ammunition (rounds) | CT_Ammo_N | Cylinder r=0.005 d=0.015 (9mm) | CT_Threat_Metal | Small dense cylinders, often clustered in box |
| Ammunition box | CT_AmmoBox | Cube 0.06x0.04x0.03 filled with CT_Ammo_N | CT_Threat_Metal | Dense rectangular cluster |
| BB/pellet gun | CT_BBGun | Simplified gun shape | CT_Metal_High (15000) | Less dense than real firearm, but similar shape |
| Starter pistol | CT_StarterPistol | Gun-shaped, thinner barrel | CT_Metal_High | Gun-shaped but barrel sealed |
| Flare gun | CT_FlareGun | Large bore barrel, plastic grip | Barrel: CT_Metal_High, Grip: CT_Fabric_Low | Wide bore distinctive |

## Knives & Sharp Objects (Threat Category: METAL)

Distinctive CT signature: thin flat metal, blade tapers to edge, handle may be composite.

| Item | Object Names | Geometry | Material | CT Signature |
|------|-------------|----------|----------|--------------|
| Kitchen knife (large) | CT_Knife_Blade, CT_Knife_Handle, CT_Knife_Tang, CT_Knife_Rivet_N | Blade: cube 0.10x0.018x0.001; Handle: cube 0.05x0.012x0.006; Tang: cube 0.05x0.008x0.002 | Blade/tang/rivets: CT_Threat_Metal; Handle: CT_Electronics | Long thin flat metal with handle |
| Pocket knife (folded) | CT_PocketKnife | Cube 0.04x0.012x0.008 | CT_Threat_Metal | Small dense rectangle when folded |
| Box cutter | CT_BoxCutter_Body, CT_BoxCutter_Blade | Body: cube 0.05x0.015x0.005; Blade: cube 0.02x0.01x0.0005 | Body: CT_Metal_High; Blade: CT_Threat_Metal | Thin blade extends from housing |
| Utility knife | CT_UtilityKnife | Cube 0.07x0.02x0.01 | CT_Metal_High + CT_Threat_Metal blade | Retractable blade in handle |
| Straight razor | CT_StraightRazor | Cube 0.06x0.015x0.003 folded | CT_Threat_Metal | Thin dense fold |
| Scissors (large) | CT_Scissors_Blade_L, CT_Scissors_Blade_R, CT_Scissors_Pivot | 2 crossed blades: cube 0.06x0.01x0.001 each; Pivot: cyl r=0.003 | CT_Threat_Metal | X-shaped metal, distinctive pivot point |
| Sword/machete | CT_Machete | Cube 0.25x0.03x0.002 | CT_Threat_Metal | Very long flat metal |
| Ice pick | CT_IcePick_Shaft, CT_IcePick_Handle | Shaft: cyl r=0.002 d=0.10; Handle: cyl r=0.012 d=0.05 | Shaft: CT_Threat_Metal; Handle: CT_Fabric_Low | Thin metal spike |
| Ceramic knife | CT_CeramicKnife | Cube 0.08x0.015x0.001 | CT_Electronics (7000) | Lower density than metal, harder to detect |
| Multi-tool (open) | CT_MultiTool | Cube 0.04x0.015x0.012 | CT_Threat_Metal | Dense multi-layered metal block |
| Throwing stars | CT_ThrowingStar_N | Flat star shape (thin disk) | CT_Threat_Metal | Small thin metal star |

## Explosives & IED Components (Threat Category: EXPLOSIVE)

Distinctive CT signature: dense cylindrical/rectangular containers, wires connecting components, timer/clock mechanism, detonator caps.

| Item | Object Names | Geometry | Material | CT Signature |
|------|-------------|----------|----------|--------------|
| Pipe bomb | CT_PipeBomb_Pipe, CT_PipeBomb_Cap_L, CT_PipeBomb_Cap_R, CT_PipeBomb_Filler | Pipe: cyl r=0.02 d=0.15; Caps: cyl r=0.022 d=0.005; Filler: cyl r=0.018 d=0.14 | Pipe/caps: CT_Metal_High; Filler: CT_Food (2000) - organic explosive is similar to organic | Metal pipe with end caps, dense fill |
| Timer/clock w/ wires | CT_Timer_Clock, CT_Timer_Battery, CT_Timer_Wire_N | Clock: cyl r=0.03 d=0.02; Battery: cube 0.02x0.01x0.04; Wires: thin cylinders r=0.001 | Clock: CT_Electronics; Battery: CT_Metal_High; Wires: CT_Metal_High | Clock mechanism + battery + wires running to other components |
| Detonator/blasting cap | CT_Detonator | Cylinder r=0.003 d=0.03 | CT_Threat_Metal | Small dense cylinder, wires attached |
| Plastic explosive block | CT_PlasticExplosive | Cube 0.08x0.05x0.02 | CT_Food (2000) - similar organic density | Uniform density block, organic range, rectangular |
| Dynamite sticks | CT_Dynamite_N | Cylinder r=0.012 d=0.10 | CT_Food (2000) + CT_Metal_High wire | Cylindrical organic with metal wire/cap |
| Improvised circuit board | CT_IED_Board, CT_IED_Components | Board: cube 0.04x0.03x0.002; Components: small cubes/cylinders | Board: CT_Electronics; Components: mixed | PCB with unusual component arrangement |
| Suspicious battery pack | CT_SusBattery | Multiple cylinders taped together | CT_Metal_High | Cluster of cylindrical cells, abnormal for consumer electronics |
| Fireworks | CT_Fireworks_N | Cylinder r=0.01 d=0.08 | CT_Food + CT_Metal_High fuse | Cylindrical with dense fuse/igniter |
| Liquid explosive | CT_LiquidExplosive | Cylinder (bottle shape) r=0.025 d=0.10 | CT_Liquid_Medium (3500) | Liquid in container, indistinguishable from water without dual-energy CT |

## Clubs & Impact Weapons (Threat Category: CONTRABAND)

| Item | Object Names | Geometry | Material | CT Signature |
|------|-------------|----------|----------|--------------|
| Brass knuckles | CT_BrassKnuckles | Torus-like with finger holes | CT_Threat_Metal | Dense small metal with holes |
| Billy club/baton | CT_Baton | Cylinder r=0.015 d=0.30 | CT_Metal_High | Long dense cylinder |
| Blackjack | CT_Blackjack | Cylinder r=0.02 d=0.15, weighted end | CT_Leather + CT_Metal_High end | Leather wrap with dense tip |
| Stun gun/taser | CT_StunGun | Cube 0.04x0.025x0.015 | CT_Electronics + CT_Metal_High probes | Electronics with two metal probes |
| Pepper spray | CT_PepperSpray | Cylinder r=0.015 d=0.06 | CT_Metal_High can + CT_Liquid_Medium | Small pressurized canister |
| Nunchucks | CT_Nunchuck_L, CT_Nunchuck_R, CT_Nunchuck_Chain | 2 cylinders r=0.012 d=0.14 + chain | Sticks: CT_Fabric_Low (wood) or CT_Metal_High; Chain: CT_Metal_High | Two rods connected by chain |
| Slingshot | CT_Slingshot_Frame, CT_Slingshot_Band | Y-frame + elastic band | Frame: CT_Metal_High; Band: CT_Underwear | Y-shaped metal fork |

## Tools (Threat Category: CONTRABAND)

| Item | Object Names | Geometry | Material | CT Signature |
|------|-------------|----------|----------|--------------|
| Crowbar | CT_Crowbar | Curved cylinder r=0.01 d=0.30 | CT_Metal_High | Long curved dense rod |
| Hammer | CT_Hammer_Head, CT_Hammer_Handle | Head: cube 0.04x0.02x0.02; Handle: cyl r=0.012 d=0.15 | Head: CT_Metal_High; Handle: CT_Fabric_Low (wood) | Dense head on lighter shaft |
| Hatchet/axe | CT_Hatchet_Head, CT_Hatchet_Handle | Head: wedge 0.06x0.04x0.01; Handle: cyl r=0.015 d=0.20 | Head: CT_Threat_Metal; Handle: CT_Fabric_Low | Dense wedge on shaft |
| Large screwdriver | CT_Screwdriver | Shaft: cyl r=0.004 d=0.15; Handle: cyl r=0.015 d=0.05 | Shaft: CT_Metal_High; Handle: CT_Liquid_Medium (plastic) | Metal shaft in plastic handle |
| Wrench | CT_Wrench | Cube 0.15x0.03x0.008 | CT_Metal_High | Long flat dense metal |
| Pliers | CT_Pliers | Two arms + pivot | CT_Metal_High | X-shape at pivot, two handles |
| Drill (cordless) | CT_Drill_Body, CT_Drill_Battery, CT_Drill_Bit | Body: cube 0.08x0.04x0.06; Battery: cube 0.04x0.04x0.03; Bit: cyl r=0.003 d=0.05 | Body: CT_Fabric_Low; Battery: CT_Metal_High; Bit: CT_Metal_High | Dense battery + motor + metal bit |
| Saw blade | CT_SawBlade | Disk r=0.08 d=0.001 with teeth | CT_Threat_Metal | Thin circular metal disk |

## Sporting Goods (Threat Category: CONTRABAND)

| Item | Object Names | Geometry | Material | CT Signature |
|------|-------------|----------|----------|--------------|
| Baseball bat | CT_BaseballBat | Cylinder r=0.03 tapered d=0.45 | CT_Fabric_Low (wood) or CT_Metal_High (aluminum) | Long cylinder, wood is faint, aluminum is bright |
| Golf club | CT_GolfClub_Head, CT_GolfClub_Shaft | Head: complex shape; Shaft: cyl r=0.005 d=0.50 | CT_Metal_High | Long thin shaft with dense head |
| Martial arts weapon | CT_MartialArts | Various shapes | CT_Metal_High or CT_Fabric_Low | Depends on material |
| Pool cue | CT_PoolCue | Long tapered cylinder d=0.60 | CT_Fabric_Low | Long wooden rod, faint |

## Suspicious Configurations (Threat Category: ANOMALY)

These aren't single items but arrangements that look suspicious in CT:

| Configuration | Description | Components | CT Signature |
|--------------|-------------|------------|--------------|
| Clock + wires + battery | Classic IED indicator | Timer mechanism with wires running to battery and container | Electronics connected by metal wire paths |
| Excessive liquid | Oversized containers, multiple bottles | Multiple cylinders exceeding 3-4-1-1 rule | Large uniform-density volumes |
| Concealed metal in organic | Metal wrapped in clothing/food | Dense object surrounded by low-density material | Bright spot embedded in faint layer |
| Modified electronics | Gutted laptop with added components | Electronics shell with unusual internal density | Expected internal structure missing/altered |
| Powder in container | Loose powder in jar/bag | Uniform granular density in container | Fine-grain density, distinct from liquids |

## Threat Item Placement Guidelines

When placing threat items in a bag:
- **Concealment**: real threats are often hidden among clothing, inside shoes, wrapped in fabric
- **Orientation**: weapons may be at odd angles to avoid profile detection
- **Disassembly**: firearm parts may be separated across the bag
- **IED placement**: timer/detonator near but not touching main charge, connected by wires
- **Use `random.uniform` for rotation** to make each placement unique
- All threat items must still be **inside the bag bounds** - they don't go in the tray
