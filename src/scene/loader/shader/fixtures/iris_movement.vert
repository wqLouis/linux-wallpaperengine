#include "common.h"

uniform mat4 g_ModelViewProjectionMatrix;
uniform float g_Time;

// [COMBO] {"material":"Follow Cursor","combo":"FOLLOWCURSOR","type":"options","default":1}
// [COMBO] {"material":"Manual Control","combo":"MANUALCONTROL","type":"options","default":0}


#if !FOLLOWCURSOR && !MANUALCONTROL // NOISE
uniform vec2 g_Scale; // {"default":"1 1","label":"Scale","linked":true,"material":"scale","range":[0.01,10.0]}
uniform vec2 g_ScaleMultiplier; // {"default":"1 1","label":"Scale Multiplier","linked":true,"material":"scalemultiplier","range":[0.01,10.0]}
uniform float g_Speed; // {"material":"speed","label":"ui_editor_properties_speed","default":1,"range":[0.01, 2.0]}
uniform float g_Rough; // {"material":"rough","label":"ui_editor_properties_smoothness","default":0.2,"range":[0.01, 1.0]}
uniform float g_NoiseAmount; // {"material":"noiseamount","label":"ui_editor_properties_noise_amount","default":0.5,"range":[0.01, 2.0]}
uniform float g_PhaseOffset; // {"material":"phase", "label":"ui_editor_properties_phase", "default":0,"range":[-1, 1]}
#endif

#if FOLLOWCURSOR && !MANUALCONTROL // FOLLOWCURSOR
uniform vec2 g_CursorScale; // {"default":"1 1","label":"Cursor Scale","linked":true,"material":"cursorscale","range":[0.01,10.0]}
uniform vec2 g_CursorScaleMultiplier; // {"default":"1 1","label":"Cursor Scale Multiplier","linked":true,"material":"cursorscalemultiplier","range":[0.01,10.0]}
uniform vec2 g_CursorScaleLimit; // {"default":"1 1","label":"Cursor Scale Limit","linked":true,"material":"cursorscalelimit","range":[0,1]}
#endif

#if MANUALCONTROL //MANUALCONTROL
uniform vec2 g_ManualScale; // {"default":"1 1","label":"Manual Scale","linked":true,"material":"manualscale","range":[0.01,10.0]}
uniform vec2 g_ManualScaleMultiplier; // {"default":"1 1","label":"Manual Scale Multiplier","linked":true,"material":"manualscalemultiplier","range":[0.01,10.0]}
uniform vec2 g_Manual_XY; // {"default":"0 0","label":"Manual XY","linked":true,"material":"manual_xy","range":[-1,1]}
#endif


#if MASK
uniform vec4 g_Texture1Resolution;
#endif

attribute vec3 a_Position;
attribute vec2 a_TexCoord;

varying vec4 v_TexCoord;
varying vec2 v_TexCoordIris;

uniform mat4 g_EffectTextureProjectionMatrixInverse;
uniform vec2 g_PointerPosition;
uniform vec2 g_PointerPositionLast;
uniform vec4 g_Texture0Resolution;

varying vec4 v_PointerUV;

void main() {
    gl_Position = mul(vec4(a_Position, 1.0), g_ModelViewProjectionMatrix);
    v_TexCoord = a_TexCoord.xyxy;

#if MASK
    v_TexCoord.zw = vec2(v_TexCoord.x * g_Texture1Resolution.z / g_Texture1Resolution.x,
                        v_TexCoord.y * g_Texture1Resolution.w / g_Texture1Resolution.y);
#endif


#if !FOLLOWCURSOR && !MANUALCONTROL // NOISE
    float time = (g_Time * g_Speed) + g_PhaseOffset;

	float lowDt = floor(time);
	vec2 motion2 = sin(1.9 * (lowDt + vec2(0, 1)));
    vec4 motion4 = sin(2.5 * (lowDt + vec4(0, 0, 1, 1)) + vec4(1, 2, 1, 2));
    vec2 moveStart = motion2.xx + motion4.xy;
    vec2 moveEnd = motion2.yy + motion4.zw;
    vec2 da = mix(moveStart, moveEnd, smoothstep(1 - g_Rough, 1, cos(frac(time) * M_PI) * -0.5 + 0.5));

    da.x += sin(time) * g_NoiseAmount;
    da.y += cos(time) * g_NoiseAmount;
	
    da *= g_Scale * 0.001 * g_ScaleMultiplier;
    v_TexCoordIris = da.xy;
#endif

#if FOLLOWCURSOR && !MANUALCONTROL // FOLLOWCURSOR
    // Adjusting cursor coordinates with layer's local transformation
	vec2 cursorPositionAdjusted = g_PointerPosition;
    cursorPositionAdjusted.y = 1.0 - cursorPositionAdjusted.y;

	cursorPositionAdjusted.x = (cursorPositionAdjusted.x - 0.5) * 2.0; // Adjust to [-1,1]
	cursorPositionAdjusted.y = (cursorPositionAdjusted.y - 0.5) * 2.0;

	vec4 transformedCursorPosition = mul(vec4(cursorPositionAdjusted, 0.0, 1.0), g_EffectTextureProjectionMatrixInverse);

	transformedCursorPosition.xy = clamp(transformedCursorPosition.xy, -g_CursorScaleLimit, g_CursorScaleLimit);
    transformedCursorPosition.x *= -1.0;

	vec2 da = transformedCursorPosition * g_CursorScale * g_CursorScaleMultiplier * 0.001;
	v_TexCoordIris = da.xy;
#endif

#endif

#if MANUALCONTROL //MANUALCONTROL
	vec2 da = g_Manual_XY * g_ManualScale * g_ManualScaleMultiplier * -0.001;
	v_TexCoordIris = da.xy;
#endif
}