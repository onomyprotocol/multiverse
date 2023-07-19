package app_test

import (
	"encoding/json"
	"testing"

	// use the interchain-security provider for the provider
	"github.com/cosmos/cosmos-sdk/simapp"
	appProvider "github.com/cosmos/interchain-security/app/provider"
	ibctesting "github.com/cosmos/interchain-security/legacy_ibc_testing/testing"
	e2e "github.com/cosmos/interchain-security/tests/e2e"
	icstestingutils "github.com/cosmos/interchain-security/testutil/ibc_testing"
	appConsumer "github.com/onomyprotocol/multiverse/app/consumer"
	"github.com/stretchr/testify/suite"
	"github.com/tendermint/spm/cosmoscmd"
	"github.com/tendermint/tendermint/libs/log"
	tmdb "github.com/tendermint/tm-db"
)

// ConsumerAppIniter implements ibctesting.AppIniter for a consumer app
func ConsumerAppIniter() (ibctesting.TestingApp, map[string]json.RawMessage) {
	encoding := cosmoscmd.MakeEncodingConfig(appConsumer.ModuleBasics)
	testApp := appConsumer.New(log.NewNopLogger(), tmdb.NewMemDB(), nil, true, map[int64]bool{},
		simapp.DefaultNodeHome, 5, encoding, simapp.EmptyAppOptions{}).(ibctesting.TestingApp)
	return testApp, appConsumer.NewDefaultGenesisState(encoding.Marshaler)
}

// Executes the standard group of ccv tests against a consumer and provider app.go implementation.
func TestCCVTestSuite(t *testing.T) {
	// Pass in concrete app types that implement the interfaces defined in /testutil/e2e/interfaces.go
	// IMPORTANT: the concrete app types passed in as type parameters here must match the
	// concrete app types returned by the relevant app initers.
	ccvSuite := e2e.NewCCVTestSuite[*appProvider.App, *appConsumer.App](
		// Pass in ibctesting.AppIniters for provider and consumer.
		icstestingutils.ProviderAppIniter, ConsumerAppIniter, []string{})

	// Run tests
	suite.Run(t, ccvSuite)
}
