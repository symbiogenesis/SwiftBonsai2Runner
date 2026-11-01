/* Host-only ABI control. These mocks never load or call a Qualcomm library. */
#include <inttypes.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "dspqueue.h"
#include "htp-ops.h"

#if !defined(_WIN32) || !defined(_M_ARM64)
#error This control must use the native Windows ARM64 public-header conditionals.
#endif

#define CHECK_SIZE(type, bytes, alignment) \
    _Static_assert(sizeof(type) == bytes, #type " size"); \
    _Static_assert(_Alignof(type) == alignment, #type " alignment")
#define CHECK_OFFSET(type, member, byte) \
    _Static_assert(offsetof(type, member) == byte, #type "." #member " offset")

CHECK_SIZE(AEEResult, 4, 4);
CHECK_SIZE(dspqueue_t, 8, 8);
CHECK_SIZE(struct dspqueue_buffer, 24, 8);
CHECK_OFFSET(struct dspqueue_buffer, fd, 0);
CHECK_OFFSET(struct dspqueue_buffer, size, 4);
CHECK_OFFSET(struct dspqueue_buffer, offset, 8);
CHECK_OFFSET(struct dspqueue_buffer, flags, 12);
CHECK_OFFSET(struct dspqueue_buffer, ptr, 16);
CHECK_OFFSET(struct dspqueue_buffer, address, 16);
CHECK_SIZE(struct htp_tensor, 56, 8);
CHECK_OFFSET(struct htp_tensor, data, 0);
CHECK_OFFSET(struct htp_tensor, size, 8);
CHECK_OFFSET(struct htp_tensor, flags, 12);
CHECK_OFFSET(struct htp_tensor, type, 16);
CHECK_OFFSET(struct htp_tensor, bi, 20);
CHECK_OFFSET(struct htp_tensor, ti, 22);
CHECK_OFFSET(struct htp_tensor, ne, 24);
CHECK_OFFSET(struct htp_tensor, nb, 40);
CHECK_SIZE(struct htp_buf_desc, 24, 8);
CHECK_OFFSET(struct htp_buf_desc, base, 0);
CHECK_OFFSET(struct htp_buf_desc, size, 8);
CHECK_OFFSET(struct htp_buf_desc, flags, 16);
CHECK_OFFSET(struct htp_buf_desc, fd, 20);
CHECK_SIZE(struct htp_op_desc, 232, 4);
CHECK_OFFSET(struct htp_op_desc, opcode, 0);
CHECK_OFFSET(struct htp_op_desc, flags, 4);
CHECK_OFFSET(struct htp_op_desc, params, 8);
CHECK_OFFSET(struct htp_op_desc, kernel_params, 72);
CHECK_OFFSET(struct htp_op_desc, src, 200);
CHECK_OFFSET(struct htp_op_desc, dst, 220);
CHECK_OFFSET(struct htp_op_desc, pad, 228);
CHECK_SIZE(struct htp_opbatch_req, 24, 8);
CHECK_OFFSET(struct htp_opbatch_req, seq, 0);
CHECK_OFFSET(struct htp_opbatch_req, n_bufs, 8);
CHECK_OFFSET(struct htp_opbatch_req, n_tensors, 12);
CHECK_OFFSET(struct htp_opbatch_req, n_ops, 16);
CHECK_OFFSET(struct htp_opbatch_req, n_traces, 20);
CHECK_SIZE(struct htp_opbatch_rsp, 88, 8);
CHECK_OFFSET(struct htp_opbatch_rsp, seq, 0);
CHECK_OFFSET(struct htp_opbatch_rsp, cycles_start, 8);
CHECK_OFFSET(struct htp_opbatch_rsp, cycles_stop, 16);
CHECK_OFFSET(struct htp_opbatch_rsp, status, 24);
CHECK_OFFSET(struct htp_opbatch_rsp, n_bufs, 28);
CHECK_OFFSET(struct htp_opbatch_rsp, n_tensors, 32);
CHECK_OFFSET(struct htp_opbatch_rsp, n_ops, 36);
CHECK_OFFSET(struct htp_opbatch_rsp, usecs, 40);
CHECK_OFFSET(struct htp_opbatch_rsp, n_traces, 44);
CHECK_SIZE(struct htp_prof_desc, 48, 4);
CHECK_OFFSET(struct htp_prof_desc, opcode, 0);
CHECK_OFFSET(struct htp_prof_desc, usecs, 4);
CHECK_OFFSET(struct htp_prof_desc, cycles_start, 8);
CHECK_OFFSET(struct htp_prof_desc, cycles_stop, 12);
CHECK_OFFSET(struct htp_prof_desc, pmu, 16);
_Static_assert(DSPQUEUE_TIMEOUT_NONE == UINT32_MAX, "infinite timeout is not zero");
_Static_assert(DSPQUEUE_PACKET_FLAG_MESSAGE == 1 && DSPQUEUE_PACKET_FLAG_BUFFERS == 2,
               "packet flags");
_Static_assert(DSPQUEUE_BUFFER_FLAG_REF == 4 && DSPQUEUE_BUFFER_FLAG_DEREF == 8 &&
               DSPQUEUE_BUFFER_FLAG_FLUSH_SENDER == 16 &&
               DSPQUEUE_BUFFER_FLAG_INVALIDATE_SENDER == 32 &&
               DSPQUEUE_BUFFER_FLAG_FLUSH_RECIPIENT == 64 &&
               DSPQUEUE_BUFFER_FLAG_INVALIDATE_RECIPIENT == 128, "buffer cache/reference flags");
_Static_assert(HTP_OP_MUL_MAT == 4 && HTP_TYPE_F32 == 0 && HTP_TYPE_Q2_0 == 42 &&
               HTP_STATUS_OK == 1, "signed operation IDs");

static unsigned char queue_marker;
static unsigned int calls;
static unsigned int unexpected;
static const uint8_t payload[8] = {1, 3, 5, 7, 9, 11, 13, 15};
static const struct dspqueue_buffer original = {
    UINT32_C(0xfedcba98), UINT32_C(0x10203040), UINT32_C(0x01020304),
    DSPQUEUE_BUFFER_FLAG_REF | DSPQUEUE_BUFFER_FLAG_FLUSH_SENDER |
    DSPQUEUE_BUFFER_FLAG_INVALIDATE_RECIPIENT, {.address = UINT64_C(0x8123456789abcdef)}
};

/* Definitions are checked against the complete public prototypes by the C compiler. */
AEEResult dspqueue_create(int domain, uint32_t flags, uint32_t req_size, uint32_t rsp_size,
                         dspqueue_callback_t packet_callback, dspqueue_callback_t error_callback,
                         void *context, dspqueue_t *queue) {
    if (calls++ != 0 || domain != 3 || flags != 0 || req_size != 4096 || rsp_size != 4096 ||
        packet_callback != NULL || error_callback != NULL || context != NULL || queue == NULL) {
        ++unexpected; return -77;
    }
    *queue = (dspqueue_t)&queue_marker;
    return 0;
}

AEEResult dspqueue_export(dspqueue_t queue, uint64_t *id) {
    if (calls++ != 1 || queue != (dspqueue_t)&queue_marker || id == NULL) {
        ++unexpected; return -77;
    }
    *id = UINT64_C(0xf102030405060708);
    return 0;
}

AEEResult dspqueue_write_noblock(dspqueue_t queue, uint32_t flags, uint32_t n,
                                struct dspqueue_buffer *buffers, uint32_t length,
                                const uint8_t *message) {
    if (calls++ != 2 || queue != (dspqueue_t)&queue_marker || flags != 3 || n != 1 ||
        buffers == NULL || length != sizeof(payload) || message == NULL ||
        memcmp(buffers, &original, sizeof(original)) != 0 || memcmp(message, payload, sizeof(payload)) != 0) {
        ++unexpected; return -77;
    }
    return 0;
}

AEEResult dspqueue_read_noblock(dspqueue_t queue, uint32_t *flags, uint32_t max_buffers,
                               uint32_t *n, struct dspqueue_buffer *buffers, uint32_t max_length,
                               uint32_t *length, uint8_t *message) {
    if (calls++ != 3 || queue != (dspqueue_t)&queue_marker || flags == NULL || n == NULL ||
        buffers == NULL || length == NULL || message == NULL || max_buffers != 1 ||
        max_length != sizeof(payload) || *flags != UINT32_C(0xcccccccc) ||
        *n != UINT32_C(0xcccccccc) || *length != UINT32_C(0xcccccccc)) {
        ++unexpected; return -77;
    }
    *flags = 3; *n = 1; *length = (uint32_t)sizeof(payload);
    *buffers = original;
    memcpy(message, payload, sizeof(payload));
    return 0;
}

AEEResult dspqueue_close(dspqueue_t queue) {
    if (calls++ != 4 || queue != (dspqueue_t)&queue_marker) { ++unexpected; return -77; }
    return 0;
}

/* Explicit boundary signatures: incompatible function pointers fail /WX compilation. */
typedef int (*create_signature)(int, uint32_t, uint32_t, uint32_t,
                                void (*)(dspqueue_t, int, void *), void (*)(dspqueue_t, int, void *),
                                void *, dspqueue_t *);
typedef int (*close_signature)(dspqueue_t);
typedef int (*export_signature)(dspqueue_t, uint64_t *);
typedef int (*write_signature)(dspqueue_t, uint32_t, uint32_t, struct dspqueue_buffer *,
                               uint32_t, const uint8_t *);
typedef int (*read_signature)(dspqueue_t, uint32_t *, uint32_t, uint32_t *,
                              struct dspqueue_buffer *, uint32_t, uint32_t *, uint8_t *);

int main(void) {
    /* The public opaque queue pointer is preserved as such, never narrowed to an int. */
    create_signature create_fn = dspqueue_create;
    close_signature close_fn = dspqueue_close;
    export_signature export_fn = dspqueue_export;
    write_signature write_fn = dspqueue_write_noblock;
    read_signature read_fn = dspqueue_read_noblock;
    dspqueue_t queue = NULL;
    uint64_t id = 0;
    uint32_t flags = UINT32_C(0xcccccccc), n = UINT32_C(0xcccccccc), length = UINT32_C(0xcccccccc);
    struct dspqueue_buffer sent = original, received;
    uint8_t message[8];
    memset(&received, 0xcc, sizeof(received));
    memset(message, 0xcc, sizeof(message));
    int result = create_fn(3, 0, 4096, 4096, NULL, NULL, NULL, &queue);
    result |= export_fn(queue, &id);
    result |= write_fn(queue, 3, 1, &sent, (uint32_t)sizeof(payload), payload);
    result |= read_fn(queue, &flags, 1, &n, &received, (uint32_t)sizeof(message), &length, message);
    result |= close_fn(queue);
    if (id != UINT64_C(0xf102030405060708) || flags != 3 || n != 1 || length != 8 ||
        memcmp(&received, &original, sizeof(original)) != 0 || memcmp(message, payload, sizeof(payload)) != 0) {
        ++unexpected;
    }
    printf("{\"schemaVersion\":1,\"kind\":\"windows-arm64-public-queue-abi-control\","
           "\"queueBuffer\":{\"bytes\":%zu,\"alignment\":%zu,\"fdOffset\":%zu,"
           "\"sizeOffset\":%zu,\"offsetOffset\":%zu,\"flagsOffset\":%zu,\"addressOffset\":%zu},"
           "\"htp\":{\"tensorBytes\":%zu,\"bufferBytes\":%zu,\"operationBytes\":%zu,"
           "\"requestBytes\":%zu,\"responseBytes\":%zu,\"profileBytes\":%zu},"
           "\"infiniteTimeout\":%" PRIu32 ",\"mockCalls\":%u,\"unexpectedCalls\":%u,"
           "\"returnCode\":%d,\"bufferHex\":\"",
           sizeof(original), _Alignof(struct dspqueue_buffer), offsetof(struct dspqueue_buffer, fd),
           offsetof(struct dspqueue_buffer, size), offsetof(struct dspqueue_buffer, offset),
           offsetof(struct dspqueue_buffer, flags), offsetof(struct dspqueue_buffer, address),
           sizeof(struct htp_tensor), sizeof(struct htp_buf_desc), sizeof(struct htp_op_desc),
           sizeof(struct htp_opbatch_req), sizeof(struct htp_opbatch_rsp), sizeof(struct htp_prof_desc),
           (uint32_t)DSPQUEUE_TIMEOUT_NONE, calls, unexpected, result);
    for (size_t index = 0; index < sizeof(original); ++index) {
        printf("%02x", (unsigned int)((const unsigned char *)&original)[index]);
    }
    printf("\",\"driverLoaded\":false,\"deviceExecuted\":false,\"profileEligible\":false}\n");
    return calls == 5 && unexpected == 0 && result == 0 ? 0 : 1;
}
