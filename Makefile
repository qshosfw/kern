.PHONY: all build check flash clean size info help

all: build

build:
	@./kern build

check:
	@./kern check

flash:
	@./kern flash

info:
	@./kern info

size:
	@./kern size

clean:
	@./kern clean

help:
	@./kern help
