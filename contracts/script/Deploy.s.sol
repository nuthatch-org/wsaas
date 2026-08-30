// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.27;

import {Script, console2} from "forge-std/Script.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {WebSocketDataService} from "../src/WebSocketDataService.sol";

/// @notice Deploy WebSocketDataService (UUPS upgradeable proxy) to a target network.
///
/// Deploys the implementation contract and an ERC1967Proxy, calling initialize()
/// atomically via the proxy constructor.
///
/// Usage — Arbitrum Sepolia (testnet):
///   forge script contracts/script/Deploy.s.sol \
///     --rpc-url arbitrum_sepolia \
///     --private-key $PRIVATE_KEY \
///     --broadcast \
///     --verify \
///     -vvvv
///
/// Required env vars (see .env.example):
///   PRIVATE_KEY           — deployer private key (hex, 0x-prefixed)
///   OWNER                 — governance address (owner of the proxy)
///   PAUSE_GUARDIAN        — address authorised to pause the service
///
/// Horizon addresses — Arbitrum Sepolia (421614). **Corrected 2026-08-30: all three of the
/// non-Controller entries here were wrong, and two of them wrong in the silent way.**
///
///   Controller:           0x9DB3ee191681f092607035d9BDA6e59FbEaCa695
///   HorizonStaking:       0x865365C425f3A593Ffe698D9c4E6707D14d51e08
///   GraphTallyCollector:  0x382863e7B662027117449bd2c49285582bbBd21B
///   PaymentsEscrow:       0x4b5D3Da463F7E076bb7CDF5030960bf123245681
///
/// What was here before, and why it matters:
///   HorizonStaking  0xFf2Ee30d… is an implementation, not the proxy. 21 kB of code and an empty
///                   EIP-1967 slot. Calling an implementation does not revert, so a service wired
///                   to one reads uninitialised storage and nothing anywhere says why.
///   PaymentsEscrow  0x09B985a2… likewise: 6.8 kB, no proxy slot.
///   Collector       0xacC71844… is the **legacy TAPCollector**. Its own eip712Domain() reports the
///                   name "TAPCollector", so receipts signed under a "GraphTallyCollector" domain
///                   would be rejected at redemption while verifying perfectly locally.
///
/// These are constructor arguments and therefore **immutable**: a contract deployed with the old
/// values cannot be corrected by an upgrade. Verify before broadcasting:
///
///   cast storage <addr> 0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc
///   cast call <collector> "eip712Domain()(bytes1,string,string,uint256,address,bytes32,uint256[])"
///
/// Horizon addresses — Arbitrum One (42161, mainnet — NOT for use yet):
///   Controller:           0x0a8491544221dd212964fbb96487467291b2C97e
///   HorizonStaking:       0x00669A4CF01450B64E8A2A20E9b1FCB71E61eF03
///   GraphTallyCollector:  0x8f69F5C07477Ac46FBc491B1E6D91E2bb0111A9e
///   PaymentsEscrow:       0xf6Fcc27aAf1fcD8B254498c9794451d82afC673E
contract Deploy is Script {
    function run() external {
        address owner_        = vm.envAddress("OWNER");
        address pauseGuardian = vm.envAddress("PAUSE_GUARDIAN");

        // Use Arbitrum Sepolia addresses by default.
        // Override via env vars if targeting a different network.
        address controller = vm.envOr(
            "GRAPH_CONTROLLER",
            address(0x9DB3ee191681f092607035d9BDA6e59FbEaCa695)
        );
        address graphTallyCollector = vm.envOr(
            "GRAPH_TALLY_COLLECTOR",
            address(0x382863e7B662027117449bd2c49285582bbBd21B)
        );

        vm.startBroadcast();

        // Deploy implementation (immutables set here, initializers disabled).
        WebSocketDataService impl = new WebSocketDataService(controller, graphTallyCollector);
        console2.log("WebSocketDataService implementation:", address(impl));

        // Deploy UUPS proxy — initialize() called atomically.
        bytes memory initData = abi.encodeCall(WebSocketDataService.initialize, (owner_, pauseGuardian));
        ERC1967Proxy proxy = new ERC1967Proxy(address(impl), initData);
        console2.log("WebSocketDataService proxy deployed at:", address(proxy));

        vm.stopBroadcast();

        console2.log("\nAdd to your config.toml and environment:");
        console2.log("CAMP_DATA_SERVICE_ADDRESS =", vm.toString(address(proxy)));
        console2.log("CAMP_DATA_SERVICE_IMPL    =", vm.toString(address(impl)));
    }
}
