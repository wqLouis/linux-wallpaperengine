mat3 squareToQuad(vec2 p0, vec2 p1, vec2 p2, vec2 p3) {
    float dx1 = p1.x - p2.x;
    float dy1 = p1.y - p2.y;
    float dx2 = p3.x - p2.x;
    float dy2 = p3.y - p2.y;
    float dx3 = p0.x - p1.x + p2.x - p3.x;
    float dy3 = p0.y - p1.y + p2.y - p3.y;

    float det = dx1 * dy2 - dy1 * dx2;
    if (abs(det) < 1e-10) {
        return mat3(1.0);
    }

    float g = (dx3 * dy2 - dy3 * dx2) / det;
    float h = (dx1 * dy3 - dy1 * dx3) / det;

    return mat3(
        p1.x - p0.x + g * p1.x,
        p3.x - p0.x + h * p3.x,
        p0.x,
        p1.y - p0.y + g * p1.y,
        p3.y - p0.y + h * p3.y,
        p0.y,
        g,
        h,
        1.0
    );
}
