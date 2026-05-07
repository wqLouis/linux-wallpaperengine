// Wallpaper Engine standard blending modes.
// BLENDMODE is set via a [COMBO] option of type "imageblending".
//
// Blend mode values:
//   0 = Normal (lerp), 1 = Add,      2 = Subtract,
//   3 = Multiply,      4 = Screen,    5 = Overlay,
//   6 = Darken,        7 = Lighten,   8 = Color Dodge,
//   9 = Color Burn,   10 = Hard Light, 11 = Soft Light,
//  12 = Difference,   13 = Exclusion

vec3 ApplyBlending(int mode, vec3 colorA, vec3 colorB, float blend) {
    vec3 result = colorA;

    if (mode == 0) { // Normal
        result = mix(colorA, colorB, blend);
    } else if (mode == 1) { // Add (Linear Dodge)
        result = colorA + colorB * blend;
    } else if (mode == 2) { // Subtract
        result = colorA - colorB * blend;
    } else if (mode == 3) { // Multiply
        result = mix(colorA, colorA * colorB, blend);
    } else if (mode == 4) { // Screen
        vec3 screen = 1.0 - (1.0 - colorA) * (1.0 - colorB);
        result = mix(colorA, screen, blend);
    } else if (mode == 5) { // Overlay
        vec3 overlay = mix(2.0 * colorA * colorB, 1.0 - 2.0 * (1.0 - colorA) * (1.0 - colorB), step(0.5, colorA));
        result = mix(colorA, overlay, blend);
    } else if (mode == 6) { // Darken
        result = mix(colorA, min(colorA, colorB), blend);
    } else if (mode == 7) { // Lighten
        result = mix(colorA, max(colorA, colorB), blend);
    } else if (mode == 8) { // Color Dodge
        vec3 dodge = colorA / max(1.0 - colorB, 0.001);
        result = mix(colorA, dodge, blend);
    } else if (mode == 9) { // Color Burn
        vec3 burn = 1.0 - (1.0 - colorA) / max(colorB, 0.001);
        result = mix(colorA, burn, blend);
    } else if (mode == 10) { // Hard Light
        vec3 hardLight = mix(2.0 * colorA * colorB, 1.0 - 2.0 * (1.0 - colorA) * (1.0 - colorB), step(0.5, colorB));
        result = mix(colorA, hardLight, blend);
    } else if (mode == 11) { // Soft Light
        vec3 softLight = mix(sqrt(colorA) * colorB * 2.0, 1.0 - (1.0 - colorA) * (1.0 - colorB) * 2.0, step(0.5, colorB));
        result = mix(colorA, softLight, blend);
    } else if (mode == 12) { // Difference
        result = mix(colorA, abs(colorA - colorB), blend);
    } else if (mode == 13) { // Exclusion
        result = mix(colorA, colorA + colorB - 2.0 * colorA * colorB, blend);
    }

    return result;
}
