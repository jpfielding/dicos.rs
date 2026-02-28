# CT Density Catalog

Material-to-density mapping and item geometry specs for all bag contents and tray items.

## CT Attenuation Physics

A CT scanner measures X-ray attenuation in **Hounsfield Units (HU)**. The stored uint16 voxel value relates to HU via:

```
HU = stored_value * RescaleSlope + RescaleIntercept
stored_value = HU - RescaleIntercept  (with slope=1.0, intercept=-1024)
stored_value = HU + 1024
```

Attenuation depends on material **density** (g/cm³) and **effective atomic number (Z_eff)**:
- Low-Z organics (C, H, O, N): fabrics, food, plastics, explosives — low HU
- Medium-Z materials (Si, Ca, P): glass, bone, ceramics — medium HU
- High-Z metals (Fe, Cu, Zn, Pb): steel, brass, copper wire — very high HU

Security CT scanners (e.g., Smiths HI-SCAN 6040 CTiX at 140kV) measure single- or dual-energy attenuation to discriminate materials.

## Density Scale

Grounded in real CT attenuation values at ~140kV. `stored = HU + 1024`.

| Stored | HU (approx) | Material | Composition | Density g/cm³ |
|--------|-------------|----------|-------------|---------------|
| 0 | -1024 | Air | N₂/O₂ | 0.001 |
| 124 | -900 | Styrofoam/padding | Polystyrene foam | 0.03 |
| 224 | -800 | Loose cotton/silk (underwear, socks) | Cellulose fiber, ~60% air | 0.05-0.10 |
| 424 | -600 | Light fabric (t-shirts, chinos) | Cotton/polyester, folded | 0.15-0.25 |
| 524 | -500 | Paper, cardboard | Cellulose | 0.2-0.4 |
| 624 | -400 | Denim, canvas | Dense woven cotton | 0.3-0.5 |
| 724 | -300 | Wood (handle, bat) | Cellulose/lignin | 0.4-0.7 |
| 824 | -200 | Leather, rubber | Protein/polymer | 0.5-0.9 |
| 924 | -100 | Fat, chocolate, wax | Hydrocarbon | 0.9 |
| 1024 | 0 | Water, aqueous liquids | H₂O | 1.00 |
| 1074 | +50 | Nylon, polyester (bag shell) | Polyamide | 1.04-1.15 |
| 1124 | +100 | Soft food (bread, fruit flesh) | ~85% water + organic | 1.0-1.1 |
| 1174 | +150 | Polypropylene (tray), hard plastic | PP, ABS, PVC | 0.9-1.4 |
| 1224 | +200 | Dense food (cheese, meat) | Protein + fat + water | 1.0-1.1 |
| 1524 | +500 | Bone-like, ceramic | Ca compounds | 1.5-2.0 |
| 2024 | +1000 | Glass (bottles, lenses) | SiO₂ | 2.2-2.5 |
| 2524 | +1500 | Fiberglass, PCB substrate | Glass + epoxy resin | 1.8-2.0 |
| 3024 | +2000 | Aluminum (cans, foil, frame) | Al, Z=13 | 2.7 |
| 4024 | +3000 | Lithium battery cells | Li + Cu + Al + electrolyte | 2.5-3.0 |
| 5024 | +4000 | Titanium | Ti, Z=22 | 4.5 |
| 7024 | +6000 | Steel, stainless (knife, tools) | Fe, Z=26 | 7.8 |
| 8024 | +7000 | Brass (keys, buckles, ammo) | Cu/Zn, Z~29 | 8.5 |
| 9024 | +8000 | Copper (wire, PCB traces) | Cu, Z=29 | 8.9 |
| 12024 | +11000 | Lead (solder, weights) | Pb, Z=82 | 11.3 |
| 15024 | +14000 | Gold (jewelry) | Au, Z=79 | 19.3 |
| 20024 | +19000 | Tungsten (counterweights) | W, Z=74 | 19.3 |

## Material Name Map

```python
DENSITY_MAP = {
    # Stored uint16 values (HU + 1024)
    'CT_Underwear':      224,   # loose cotton, ~60% air, -800 HU
    'CT_Fabric_Low':     424,   # folded t-shirts, chinos, -600 HU
    'CT_Paper':          524,   # books, cardboard, -500 HU
    'CT_Denim':          624,   # jeans, canvas, heavy fabric, -400 HU
    'CT_Wood':           724,   # wooden handles, bats, -300 HU
    'CT_Leather':        824,   # belts, purse body, shoe leather, -200 HU
    'CT_Food':           1124,  # fruit, sandwich, organic food, +100 HU
    'CT_Liquid_Medium':  1024,  # water, shampoo, drinks, 0 HU (water)
    'CT_BagShell':       1074,  # nylon/polyester bag fabric, +50 HU
    'CT_Tray':           1174,  # polypropylene tray, +150 HU
    'CT_Glass':          2024,  # glass bottles, perfume, lenses, +1000 HU
    'CT_Electronics':    2524,  # PCBs, screens, circuit boards, +1500 HU
    'CT_Aluminum':       3024,  # aluminum frame, foil, cans, +2000 HU
    'CT_Battery':        4024,  # lithium cells, power banks, +3000 HU
    'CT_Metal_High':     7024,  # steel zippers, knife blades, screws, +6000 HU
    'CT_Brass':          8024,  # keys, buckles, ammo casings, +7000 HU
    'CT_Copper':         9024,  # wire cores, connectors, +8000 HU
    'CT_Threat_Metal':   7024,  # same as steel — a knife IS steel, +6000 HU
}
```

**Notes:**
- `CT_Threat_Metal` uses the same value as `CT_Metal_High` (steel) because a steel knife blade IS steel — it has no special magical density. The threat is identified by shape/context in the TDR, not by a different attenuation value.
- Very thin objects (zipper teeth, foil, wire) may appear brighter than their bulk density suggests because partial-volume averaging with air raises apparent HU at the boundary.
- Dual-energy CT can separate materials by Z_eff even when they have similar density (e.g., organic explosive vs. cheese vs. plastic).

## Item Catalog

### Clothing

| Item | Object Name | Geometry | Material | Notes |
|------|------------|----------|----------|-------|
| Jeans (folded) | CT_Jeans_Folded | Cube 0.14x0.13x0.008 | CT_Denim | Add rivets (cyl r=0.003) + zipper (cyl r=0.0015) as CT_Metal_High |
| Chinos (folded) | CT_Chinos_Folded | Cube 0.13x0.12x0.006 | CT_Fabric_Low | |
| T-shirt (rolled) | CT_TShirt_Rolled_N | Cylinder r=0.025 d=0.22 horiz | CT_Fabric_Low | |
| T-shirt (folded) | CT_TShirt_Folded_N | Cube 0.12x0.10x0.005 | CT_Fabric_Low | |
| Button-down shirt | CT_ButtonDown | Cube 0.12x0.11x0.005 | CT_Fabric_Low | Add buttons (cyl r=0.003) as CT_Liquid_Medium |
| Dress shirt | CT_DressShirt | Cube 0.12x0.11x0.006 | CT_Fabric_Low | Collar stays as tiny CT_Metal_High rects |
| Hoodie/sweater | CT_Hoodie | Cube 0.13x0.13x0.015 | CT_Fabric_Low | Add zipper + aglets as CT_Metal_High |
| Underwear | CT_Underwear_N | Cube 0.05x0.04x0.003 | CT_Underwear | Stack 3-5, elastic waistband torus |
| Socks (rolled pair) | CT_SockPair_N | UV sphere r=0.018 squash=(1,1,0.7) | CT_Underwear | |
| Swimsuit | CT_Swimsuit | Cube 0.08x0.06x0.003 | CT_Fabric_Low | |
| Scarf/wrap | CT_Scarf | Cylinder r=0.04 d=0.08 (rolled) | CT_Fabric_Low | |
| Belt | CT_Belt | Torus major=0.05 minor=0.005 | CT_Leather | Add CT_BeltBuckle cube 0.03x0.02x0.005 as CT_Metal_High |

### Electronics

| Item | Object Name | Geometry | Material | Notes |
|------|------------|----------|----------|-------|
| Laptop | CT_Laptop | Cube 0.005x0.14x0.09 | CT_Electronics | Add CT_Battery cube for Li-ion cell |
| Tablet/iPad | CT_Tablet | Cube 0.004x0.09x0.065 | CT_Electronics | Add battery as CT_Battery |
| Phone | CT_Phone | Cube 0.003x0.035x0.07 | CT_Electronics | Add battery as CT_Battery |
| Power bank | CT_PowerBank | Cube 0.035x0.02x0.008 | CT_Electronics | Add CT_Battery cylinder for Li-ion cell |
| Earbuds case | CT_EarbudsCase | UV sphere r=0.015 squash | CT_Electronics | Add battery as CT_Battery |
| USB cable (coiled) | CT_USBCable | Torus major=0.03 minor=0.002 | CT_Copper | Copper wire core |
| Power adapter | CT_PowerAdapter | Cube 0.02x0.015x0.01 | CT_Electronics | |
| Camera body | CT_Camera | Cube 0.06x0.04x0.035 | CT_Electronics | Dense internals |
| Camera lens | CT_CameraLens | Cylinder r=0.025 d=0.04 | CT_Glass | Glass elements |
| E-reader | CT_EReader | Cube 0.004x0.08x0.06 | CT_Electronics | Thin battery |
| Portable speaker | CT_Speaker | Cylinder r=0.03 d=0.08 | CT_Electronics | |
| Mouse | CT_Mouse | UV sphere r=0.025 squash=(1.5,1,0.6) | CT_Electronics | |
| Laptop charger | CT_LaptopCharger | Cube 0.04x0.04x0.015 | CT_Electronics | Heavy brick |

### Toiletries & Cosmetics

| Item | Object Name | Geometry | Material | Notes |
|------|------------|----------|----------|-------|
| Toiletry bag | CT_ToiletryBag | Cube 0.08x0.04x0.04 | CT_Fabric_Low | |
| Shampoo bottle | CT_Shampoo | Cylinder r=0.012 d=0.06 | CT_Liquid_Medium | |
| Toothpaste | CT_Toothpaste | Cylinder r=0.008 d=0.05 horiz | CT_Liquid_Medium | Metal cap |
| Deodorant | CT_Deodorant | Cylinder r=0.015 d=0.05 | CT_Liquid_Medium | |
| Perfume/cologne | CT_Perfume | Cube 0.012x0.012x0.02 | CT_Glass | Glass bottle |
| Lipstick | CT_Lipstick | Cylinder r=0.008 d=0.04 | CT_Metal_High | Metal tube |
| Compact mirror | CT_Compact | Cylinder r=0.025 d=0.008 | CT_Electronics | Mirror + hinge |
| Razor | CT_Razor | Cube 0.04x0.02x0.01 | CT_Liquid_Medium | Metal blade as CT_Metal_High |
| Hair brush | CT_Hairbrush | Cube 0.04x0.03x0.02 | CT_Fabric_Low | |
| Sunscreen | CT_Sunscreen | Cylinder r=0.02 d=0.06 | CT_Liquid_Medium | |
| Hand sanitizer | CT_Sanitizer | Cylinder r=0.015 d=0.04 | CT_Liquid_Medium | |
| Medication bottle | CT_MedBottle | Cylinder r=0.012 d=0.04 | CT_Liquid_Medium | |

### Food & Drink

| Item | Object Name | Geometry | Material | Notes |
|------|------------|----------|----------|-------|
| Water bottle | CT_WaterBottle | Cylinder r=0.03 d=0.18 | CT_Liquid_Medium | Metal cap CT_Metal_High |
| Sandwich | CT_Sandwich | Cube 0.06x0.04x0.025 | CT_Food | |
| Snack bars | CT_SnackBar_N | Cube 0.04x0.015x0.008 | CT_Food | Foil wrapper CT_Aluminum |
| Apple/fruit | CT_Apple | UV sphere r=0.035 | CT_Food | |
| Trail mix | CT_TrailMix | Cube 0.04x0.03x0.03 | CT_Food | |
| Gum/mints | CT_Gum | Cube 0.03x0.02x0.008 | CT_Food | |
| Candy bag | CT_Candy | Cube 0.05x0.03x0.02 | CT_Food | |
| Thermos | CT_Thermos | Cylinder r=0.035 d=0.20 | CT_Metal_High | Double-wall steel |
| Baby formula | CT_Formula | Cylinder r=0.03 d=0.08 | CT_Liquid_Medium | |
| Baby food jar | CT_BabyFood | Cylinder r=0.025 d=0.04 | CT_Glass | Glass jar |

### Shoes

| Item | Object Name | Geometry | Material | Notes |
|------|------------|----------|----------|-------|
| Shoe upper | CT_Shoe_N | Cube 0.06x0.045x0.025 | CT_Fabric_Low | |
| Shoe sole | CT_Sole_N | Cube 0.065x0.048x0.008 | CT_Liquid_Medium | Rubber |
| Shoe eyelets | CT_Eyelet_N | UV sphere r=0.002 | CT_Metal_High | 4-6 per shoe |
| Boot | CT_Boot_N | Cube 0.08x0.05x0.04 | CT_Leather | Taller, thicker sole |
| Sandal | CT_Sandal_N | Cube 0.06x0.04x0.008 | CT_Liquid_Medium | Mostly sole |

### Bag Hardware (Carry-On Roller)

| Item | Object Name | Geometry | Material |
|------|------------|----------|----------|
| Handle frame L/R | CT_HandleFrame_L/R | Cylinder r=0.008 d=0.20 | CT_Metal_High |
| Handle brace | CT_HandleBrace | Cylinder r=0.006 d=0.12 horiz | CT_Metal_High |
| Internal frame | CT_InternalFrame | Cube 0.25x0.002x0.10 | CT_Metal_High |
| Wheels | CT_Wheel_N | UV sphere r=0.012 | CT_Metal_High |
| Top zipper | CT_Zipper_Top | Cylinder r=0.002, bag length | CT_Metal_High |
| Side zipper | CT_Zipper_Side | Cylinder r=0.002, pocket | CT_Metal_High |
| Zipper pulls | CT_ZipperPull_N | UV sphere r=0.004 | CT_Metal_High |

### Loose Items / Misc

| Item | Object Name | Geometry | Material | Notes |
|------|------------|----------|----------|-------|
| Keys on ring | CT_Keyring + CT_Key_N | Torus + cubes | CT_Metal_High | |
| Coins | CT_Coin_N | Cylinder r=0.01 d=0.001 | CT_Metal_High | Random rotation |
| Book | CT_Book | Cube 0.06x0.04x0.01 | CT_Paper | |
| Notebook | CT_Notebook | Cube 0.07x0.05x0.008 | CT_Paper | Wire spiral as CT_Metal_High |
| Pen | CT_Pen | Cylinder r=0.004 d=0.06 | CT_Metal_High | |
| Wallet | CT_Wallet | Cube 0.015x0.055x0.045 | CT_Leather | Credit cards as thin CT_Metal_High |
| Passport | CT_Passport | Cube 0.06x0.04x0.005 | CT_Paper | RFID chip as CT_Electronics |
| Umbrella (folded) | CT_Umbrella | Cylinder r=0.02 d=0.15 | CT_Fabric_Low | Metal shaft CT_Metal_High |
| Toy (child) | CT_Toy | UV sphere r=0.03 | CT_Liquid_Medium | Plastic |
| Diaper pack | CT_Diapers | Cube 0.08x0.06x0.04 | CT_Fabric_Low | |

### Loose Tray Items (not inside bag)

| Item | Object Name | Geometry | Material | Notes |
|------|------------|----------|----------|-------|
| Watch (metal) | Tray_Watch | Torus major=0.02 minor=0.004 | CT_Metal_High | Flat on tray floor |
| Watch (leather) | Tray_Watch | Torus (band CT_Leather) + cylinder (face CT_Electronics) | Mixed | |
| Phone | Tray_Phone | Cube 0.003x0.035x0.075 | CT_Electronics | Flat, add battery |
| Laptop (separate) | Tray_Laptop | Cube 0.01x0.17x0.12 | CT_Electronics | TSA requires separate bin |
| Tablet (separate) | Tray_Tablet | Cube 0.006x0.12x0.085 | CT_Electronics | |
| Belt | Tray_Belt | Coiled torus | CT_Leather | Buckle CT_Metal_High |
| Shoes (pair) | Tray_Shoe_N | See shoes | Mixed | Side by side on tray floor |
| Wallet | Tray_Wallet | Cube 0.015x0.055x0.045 | CT_Leather | |
| Sunglasses | Tray_Sunglasses | Curved thin cube | CT_Glass | Hinge CT_Metal_High |
| Baseball cap | Tray_Cap | Hemisphere r=0.07 | CT_Fabric_Low | Metal button CT_Metal_High |
| Jacket (folded) | Tray_Jacket | Cube 0.15x0.12x0.04 | CT_Fabric_Low | Metal zippers |

### Threat Items (Optional)

| Item | Object Name | Geometry | Material | Notes |
|------|------------|----------|----------|-------|
| Knife blade | CT_Knife_Blade | Cube 0.10x0.018x0.001 | CT_Threat_Metal | Angled diagonally |
| Knife tip | CT_Knife_Tip | Cone r1=0.018 r2=0 d=0.03 flat | CT_Threat_Metal | |
| Knife handle | CT_Knife_Handle | Cube 0.05x0.012x0.006 | CT_Electronics | Composite |
| Knife tang | CT_Knife_Tang | Cube 0.05x0.008x0.002 | CT_Threat_Metal | |
| Knife rivets | CT_Knife_Rivet_N | Cylinder r=0.003 d=0.013 | CT_Threat_Metal | 3 per handle |
| Box cutter | CT_BoxCutter | Cube 0.04x0.01x0.005 | CT_Threat_Metal | Small blade |
| Scissors | CT_Scissors | 2 crossed blades | CT_Threat_Metal | |
| Multi-tool | CT_MultiTool | Cube 0.04x0.012x0.008 | CT_Threat_Metal | Very dense |
| Firearm | CT_Firearm | L-shaped cube composite | CT_Threat_Metal | Highly distinctive |

### Appliances

| Item | Object Name | Geometry | Material | Notes |
|------|------------|----------|----------|-------|
| Hair dryer barrel | CT_HairDryer_Barrel | Cylinder r=0.025 d=0.14 | CT_Fabric_Low | Plastic |
| Hair dryer motor | CT_HairDryer_Motor | Cylinder r=0.015 d=0.04 | CT_Metal_High | Dense |
| Hair dryer coils | CT_HairDryer_Coil_N | Torus major=0.018 minor=0.002 | CT_Metal_High | |
| Hair dryer handle | CT_HairDryer_Handle | Cylinder r=0.015 d=0.07 | CT_Fabric_Low | |
| Hair dryer cord | CT_HairDryer_Cord | Torus major=0.02 minor=0.003 | CT_Metal_High | |
| Curling iron | CT_CurlingIron | Cylinder r=0.012 d=0.20 | CT_Metal_High | Metal barrel |
| Electric shaver | CT_Shaver | Cube 0.03x0.025x0.06 | CT_Electronics | Motor inside |
