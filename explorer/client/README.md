# SuiExplorer Client

# Set Up

**Requirements**: Node 14.0.0 or later version

In the project directory, run:

### `yarn install`

Before running any of the following scripts `yarn install` must run in order to install the necessary dependencies.

# How to Switch Environment

The purpose of the SuiExplorer Client is to present data extracted from a real or theoretical Sui Network.

What the 'Sui Network' is varies according to the environment variable `REACT_APP_DATA`.

When running most of the below yarn commands, the SuiExplorer Client will extract and present data from the Sui Network connected to the URL https://demo-rpc.sui.io.

If the environment variable `REACT_APP_DATA` is set to `static`, then the SuiExplorer will instead pull data from a local, static JSON dataset that can be found at `./src/utils/static/mock_data.json`.

For example, suppose we wish to locally run the website using the static JSON dataset and not the API, then we could run the following:

```bash
REACT_APP_DATA=static yarn start
```

Note that the commands `yarn test` and `yarn start:static` are the exceptions. Here the SuiExplorer will instead use the static JSON dataset. The tests have been written to specifically check the UI and not the API connection and so use the static JSON dataset.

## Yarn Commands and what they do

### `yarn start`

Runs the app as connected to the API at https://demo-rpc.sui.io.

Open http://localhost:3000 to view it in the browser.

The page will reload if you make edits. You will also see any lint errors in the console.

### `yarn start:static`

Runs the app as connected to the static JSON dataset with `REACT_APP_DATA` set to `static`.

Open http://localhost:8080 to view it in the browser.

The page will reload when edits are made. You can run `yarn start` and `yarn start:static` at the same time because they use different ports.

### `yarn test`

This runs a series of end-to-end browser tests using the website as connected to the static JSON dataset. This command is run by the GitHub checks. The tests must pass before merging a branch into main.

### `yarn build`

Builds the app for production to the `build` folder.

It bundles React in production mode and optimizes the build for the best performance.

### `yarn lint`

Run linting check (prettier/eslint/stylelint).

### `yarn lint:fix`

Run linting check but also try to fix any issues.

## Deployment

For guidance on deployment, plese see here: https://create-react-app.dev/docs/deployment/

Because of the addition of `react-router`, further changes will be needed that depend on the exact infrastructure used. Please consult section **Serving Apps with Client-Side Routing**.
