// Draws the camera preview with WebGL2. Adapted from the preview script in `dioxus-cameras`,
// which redraws at the display's refresh rate and downloads a whole frame on every poll.
// This one passes the last frame it drew as `after`, so polls between camera frames come
// back as a bare header, and it draws only when there is something new to show.
(function () {
    const VS = `#version 300 es
in vec2 position;
out vec2 v_uv;
void main() {
    v_uv = vec2((position.x + 1.0) * 0.5, 1.0 - (position.y + 1.0) * 0.5);
    gl_Position = vec4(position, 0.0, 1.0);
}`;

    const FS = `#version 300 es
precision mediump float;
in vec2 v_uv;
out vec4 frag_color;
uniform sampler2D rgba_tex;
uniform vec2 u_crop;
uniform float u_swap_rb;
void main() {
    vec4 c = texture(rgba_tex, v_uv * u_crop);
    frag_color = vec4(mix(c.rgb, c.bgr, u_swap_rb), 1.0);
}`;

    const HEADER = 24;
    const FORMAT_BGRA = 2;
    const FORMAT_RGBA = 3;
    const POLL_MS = 28;

    function setupCanvas(canvas) {
        if (canvas._camerasInit) return;
        canvas._camerasInit = true;
        const url = canvas.dataset.previewUrl;
        const gl = url && canvas.getContext("webgl2", { alpha: false, antialias: false });
        if (!gl) return;

        function compile(src, type) {
            const shader = gl.createShader(type);
            gl.shaderSource(shader, src);
            gl.compileShader(shader);
            return shader;
        }
        const program = gl.createProgram();
        gl.attachShader(program, compile(VS, gl.VERTEX_SHADER));
        gl.attachShader(program, compile(FS, gl.FRAGMENT_SHADER));
        gl.linkProgram(program);
        if (!gl.getProgramParameter(program, gl.LINK_STATUS)) return;
        gl.useProgram(program);

        gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
        gl.bufferData(
            gl.ARRAY_BUFFER,
            new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]),
            gl.STATIC_DRAW,
        );
        const position = gl.getAttribLocation(program, "position");
        gl.enableVertexAttribArray(position);
        gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);

        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, gl.createTexture());
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
        gl.uniform1i(gl.getUniformLocation(program, "rgba_tex"), 0);
        const crop = gl.getUniformLocation(program, "u_crop");
        const swap = gl.getUniformLocation(program, "u_swap_rb");

        let fetching = false;
        let lastFetchStart = 0;
        let counter = null;
        let texture = { w: 0, h: 0 };
        let showing = false;
        let dirty = false;

        function upload(format, width, height, stride, pixels) {
            const rowPixels = stride >> 2;
            if (pixels.byteLength < stride * height) return;
            pixels = pixels.subarray(0, stride * height);
            if (texture.w !== rowPixels || texture.h !== height) {
                gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, rowPixels, height, 0,
                    gl.RGBA, gl.UNSIGNED_BYTE, pixels);
                texture = { w: rowPixels, h: height };
            } else {
                gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, rowPixels, height,
                    gl.RGBA, gl.UNSIGNED_BYTE, pixels);
            }
            gl.uniform2f(crop, width / rowPixels, 1.0);
            gl.uniform1f(swap, format === FORMAT_BGRA ? 1.0 : 0.0);
            showing = true;
        }

        async function poll() {
            fetching = true;
            try {
                const query = counter === null ? "" : `?after=${counter}`;
                const response = await fetch(url + query, { cache: "no-store" });
                if (!response.ok) return;
                const buffer = await response.arrayBuffer();
                if (buffer.byteLength < HEADER) return;
                const view = new DataView(buffer);
                if (view.getUint32(0, false) !== 0x43414d53) return;
                const next = view.getUint32(20, true);
                if (next === counter) return;
                counter = next;
                const format = view.getUint8(5);
                const width = view.getUint32(8, true);
                const height = view.getUint32(12, true);
                const stride = view.getUint32(16, true);
                showing = false;
                if ((format === FORMAT_BGRA || format === FORMAT_RGBA) && width && height) {
                    upload(format, width, height, stride, new Uint8Array(buffer, HEADER));
                }
                dirty = true;
            } catch (_) {
            } finally {
                fetching = false;
            }
        }

        function resize() {
            const parent = canvas.parentElement;
            if (!parent) return;
            const rect = parent.getBoundingClientRect();
            const ratio = window.devicePixelRatio || 1;
            const w = Math.max(1, Math.floor(rect.width * ratio));
            const h = Math.max(1, Math.floor(rect.height * ratio));
            if (canvas.width !== w || canvas.height !== h) {
                canvas.width = w;
                canvas.height = h;
                dirty = true;
            }
        }

        function render() {
            resize();
            if (!dirty) return;
            dirty = false;
            gl.viewport(0, 0, canvas.width, canvas.height);
            gl.clearColor(0, 0, 0, 1);
            gl.clear(gl.COLOR_BUFFER_BIT);
            if (showing) gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
        }

        function loop() {
            if (!canvas.isConnected) return;
            const now = performance.now();
            if (!fetching && now - lastFetchStart > POLL_MS) {
                lastFetchStart = now;
                poll();
            }
            render();
            requestAnimationFrame(loop);
        }
        requestAnimationFrame(loop);
    }

    function scan() {
        document.querySelectorAll("canvas[data-stream-id]").forEach(setupCanvas);
    }

    new MutationObserver(scan).observe(document.body, { childList: true, subtree: true });
    scan();
})();
