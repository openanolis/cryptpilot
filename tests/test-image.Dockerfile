# Dockerfile for building test image with qcow2
# This image contains a qcow2 test image for cryptpilot-convert integration tests

FROM scratch

# Copy the qcow2 image into the container
COPY test-image.qcow2 /image.qcow2
