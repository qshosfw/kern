.PHONY: all build check test pkg abi size bloat lint fmt flash info clean help

all: build

build:
	@./kern build

check:
	@./kern check

test:
	@./kern test

pkg:
	@./kern pkg

abi:
	@./kern abi --check

size:
	@./kern size

bloat:
	@./kern bloat report

lint:
	@./kern lint report

fmt:
	@./kern fmt

flash:
	@./kern flash

info:
	@./kern info

clean:
	@./kern clean

help:
	@./kern help
